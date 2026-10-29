use std::ffi::{CString, c_char, c_int, c_long, c_ulong, c_void};

const EXPOSURE_MASK: c_long = 1 << 15;
const BUTTON_PRESS_MASK: c_long = 1 << 2;
const KEY_PRESS_MASK: c_long = 1;
const EXPOSE: c_int = 12;
const BUTTON_PRESS: c_int = 4;
const KEY_PRESS: c_int = 2;
const WINDOW_WIDTH: u32 = 640;
const WINDOW_HEIGHT: u32 = 360;

type Display = c_void;
type Window = c_ulong;
type GraphicsContext = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy)]
struct XKeyEvent {
    event_type: c_int,
    serial: c_ulong,
    send_event: c_int,
    display: *mut Display,
    window: Window,
    root: Window,
    subwindow: Window,
    time: c_ulong,
    x: c_int,
    y: c_int,
    root_x: c_int,
    root_y: c_int,
    state: u32,
    keycode: u32,
    same_screen: c_int,
}

#[repr(C)]
union XEvent {
    event_type: c_int,
    key: XKeyEvent,
    padding: [c_long; 24],
}

#[link(name = "dl")]
unsafe extern "C" {
    fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlerror() -> *const c_char;
}

struct X11 {
    _library: *mut c_void,
    open_display: unsafe extern "C" fn(*const c_char) -> *mut Display,
    default_screen: unsafe extern "C" fn(*mut Display) -> c_int,
    root_window: unsafe extern "C" fn(*mut Display, c_int) -> Window,
    create_simple_window: unsafe extern "C" fn(
        *mut Display,
        Window,
        c_int,
        c_int,
        u32,
        u32,
        u32,
        c_ulong,
        c_ulong,
    ) -> Window,
    store_name: unsafe extern "C" fn(*mut Display, Window, *const c_char) -> c_int,
    select_input: unsafe extern "C" fn(*mut Display, Window, c_long) -> c_int,
    map_window: unsafe extern "C" fn(*mut Display, Window) -> c_int,
    create_gc: unsafe extern "C" fn(*mut Display, Window, c_ulong, *mut c_void) -> GraphicsContext,
    load_font: unsafe extern "C" fn(*mut Display, *const c_char) -> c_ulong,
    set_font: unsafe extern "C" fn(*mut Display, GraphicsContext, c_ulong) -> c_int,
    set_foreground: unsafe extern "C" fn(*mut Display, GraphicsContext, c_ulong) -> c_int,
    clear_window: unsafe extern "C" fn(*mut Display, Window) -> c_int,
    draw_string: unsafe extern "C" fn(
        *mut Display,
        Window,
        GraphicsContext,
        c_int,
        c_int,
        *const c_char,
        c_int,
    ) -> c_int,
    next_event: unsafe extern "C" fn(*mut Display, *mut XEvent) -> c_int,
    lookup_string: unsafe extern "C" fn(
        *mut XKeyEvent,
        *mut c_char,
        c_int,
        *mut c_ulong,
        *mut c_void,
    ) -> c_int,
    flush: unsafe extern "C" fn(*mut Display) -> c_int,
}

pub(super) enum Event {
    Expose,
    Click,
    Text(String),
    Ignore,
}

pub(super) struct Client {
    api: X11,
    display: *mut Display,
    window: Window,
    gc: GraphicsContext,
}

impl Client {
    pub(super) fn open() -> Result<Self, String> {
        let api = X11::load()?;
        // SAFETY: XOpenDisplay returns a live handle for the inherited private Xvfb display.
        let display = unsafe { (api.open_display)(std::ptr::null()) };
        if display.is_null() {
            return Err("XOpenDisplay could not connect to the private display".into());
        }
        // SAFETY: the display pointer is live until this process exits.
        let screen = unsafe { (api.default_screen)(display) };
        // SAFETY: screen belongs to the live X11 display.
        let root = unsafe { (api.root_window)(display, screen) };
        // SAFETY: geometry, colors, and handles are valid XCreateSimpleWindow inputs.
        let window = unsafe {
            (api.create_simple_window)(
                display,
                root,
                0,
                0,
                WINDOW_WIDTH,
                WINDOW_HEIGHT,
                1,
                0,
                0x00f3f4f6,
            )
        };
        // SAFETY: select only the event classes handled by this fixture.
        unsafe {
            (api.select_input)(
                display,
                window,
                EXPOSURE_MASK | BUTTON_PRESS_MASK | KEY_PRESS_MASK,
            );
        }
        // SAFETY: null GC values are valid with a zero value mask.
        let gc = unsafe { (api.create_gc)(display, window, 0, std::ptr::null_mut()) };
        if gc.is_null() {
            return Err("XCreateGC could not create the app drawing context".into());
        }
        // SAFETY: the Xvfb image provides the core fixed font used by the fixture.
        let font = unsafe { (api.load_font)(display, c"fixed".as_ptr()) };
        if font == 0 {
            return Err("the private display does not provide its fixed core font".into());
        }
        // SAFETY: Xlib accepts this NUL-terminated title and live window.
        unsafe {
            (api.set_font)(display, gc, font);
            (api.store_name)(display, window, c"Target:Ready".as_ptr());
            (api.map_window)(display, window);
            (api.set_foreground)(display, gc, 0x00111827);
            (api.flush)(display);
        }
        Ok(Self {
            api,
            display,
            window,
            gc,
        })
    }

    pub(super) fn next_event(&self) -> Result<Event, String> {
        let mut event: XEvent = unsafe { std::mem::zeroed() };
        // SAFETY: event has the ABI-sized storage required by XNextEvent.
        unsafe { (self.api.next_event)(self.display, &mut event) };
        // SAFETY: the tag identifies the active XEvent union field.
        Ok(match unsafe { event.event_type } {
            EXPOSE => Event::Expose,
            BUTTON_PRESS => Event::Click,
            KEY_PRESS => {
                // SAFETY: the event tag selects the XKeyEvent union member.
                let mut key = unsafe { event.key };
                let mut bytes = [0 as c_char; 16];
                let mut keysym = 0;
                // SAFETY: buffers use Xlib's ABI and remain valid for this call.
                let count = unsafe {
                    (self.api.lookup_string)(
                        &mut key,
                        bytes.as_mut_ptr(),
                        bytes.len() as c_int,
                        &mut keysym,
                        std::ptr::null_mut(),
                    )
                };
                if count <= 0 {
                    Event::Ignore
                } else {
                    // SAFETY: XLookupString initialized the reported number of bytes.
                    let text = unsafe {
                        std::slice::from_raw_parts(bytes.as_ptr().cast::<u8>(), count as usize)
                    };
                    Event::Text(String::from_utf8_lossy(text).into_owned())
                }
            }
            _ => Event::Ignore,
        })
    }

    pub(super) fn render(&self, title: &str) -> Result<(), String> {
        let title = CString::new(title).map_err(|_| "app title contains a NUL byte")?;
        // SAFETY: all Xlib handles are live and the title string is NUL-terminated.
        unsafe {
            (self.api.store_name)(self.display, self.window, title.as_ptr());
            (self.api.clear_window)(self.display, self.window);
        }
        self.draw_line(32, 42, "Rust desktop task fixture");
        self.draw_line(32, 78, "Click anywhere, then type a message");
        self.draw_line(32, 132, title.to_str().unwrap_or_default());
        // SAFETY: this display was opened by the current process.
        unsafe { (self.api.flush)(self.display) };
        Ok(())
    }

    fn draw_line(&self, x: c_int, y: c_int, value: &str) {
        let Ok(value) = CString::new(value) else {
            return;
        };
        // SAFETY: Xlib handles are live and the C string remains valid for the call.
        unsafe {
            (self.api.draw_string)(
                self.display,
                self.window,
                self.gc,
                x,
                y,
                value.as_ptr(),
                value.as_bytes().len() as c_int,
            );
        }
    }
}

impl X11 {
    fn load() -> Result<Self, String> {
        let library_name = CString::new("libX11.so.6").unwrap();
        // SAFETY: this fixed system library remains loaded for this process lifetime.
        let library = unsafe { dlopen(library_name.as_ptr(), 2) };
        if library.is_null() {
            return Err(dynamic_loader_error());
        }
        // SAFETY: names and function signatures below match the public Xlib API.
        unsafe {
            Ok(Self {
                _library: library,
                open_display: symbol(library, "XOpenDisplay")?,
                default_screen: symbol(library, "XDefaultScreen")?,
                root_window: symbol(library, "XRootWindow")?,
                create_simple_window: symbol(library, "XCreateSimpleWindow")?,
                store_name: symbol(library, "XStoreName")?,
                select_input: symbol(library, "XSelectInput")?,
                map_window: symbol(library, "XMapWindow")?,
                create_gc: symbol(library, "XCreateGC")?,
                load_font: symbol(library, "XLoadFont")?,
                set_font: symbol(library, "XSetFont")?,
                set_foreground: symbol(library, "XSetForeground")?,
                clear_window: symbol(library, "XClearWindow")?,
                draw_string: symbol(library, "XDrawString")?,
                next_event: symbol(library, "XNextEvent")?,
                lookup_string: symbol(library, "XLookupString")?,
                flush: symbol(library, "XFlush")?,
            })
        }
    }
}

unsafe fn symbol<T: Copy>(library: *mut c_void, name: &str) -> Result<T, String> {
    let name = CString::new(name).unwrap();
    // SAFETY: library came from dlopen and name is a valid C string.
    let address = unsafe { dlsym(library, name.as_ptr()) };
    if address.is_null() {
        return Err(dynamic_loader_error());
    }
    // SAFETY: callers request function pointers for known Xlib symbols only.
    Ok(unsafe { std::mem::transmute_copy(&address) })
}

fn dynamic_loader_error() -> String {
    // SAFETY: dlerror returns a process-owned C string or null.
    let error = unsafe { dlerror() };
    if error.is_null() {
        "libX11.so.6 could not be loaded".into()
    } else {
        // SAFETY: a non-null dlerror result is a valid NUL-terminated string.
        unsafe { std::ffi::CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned()
    }
}
