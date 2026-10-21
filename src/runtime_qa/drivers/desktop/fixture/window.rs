use x11rb::{
    connection::Connection,
    protocol::xproto::{
        AtomEnum, ConnectionExt, CreateGCAux, CreateWindowAux, EventMask, InputFocus, PropMode,
        Rectangle, WindowClass,
    },
    rust_connection::RustConnection,
    wrapper::ConnectionExt as _,
};

const WIDTH: u16 = 640;
const HEIGHT: u16 = 360;

pub(crate) fn run() -> Result<(), String> {
    let ignore_click = std::env::var("QA_FIXTURE_IGNORE_CLICK").as_deref() == Ok("1");
    let hide_control = std::env::var("QA_FIXTURE_HIDE_CONTROL").as_deref() == Ok("1");
    let (connection, screen) = RustConnection::connect(Some(":99"))
        .map_err(|error| format!("fixture could not connect to private Xvfb: {error}"))?;
    let screen_info = &connection.setup().roots[screen];
    let window = connection.generate_id().map_err(x11_error)?;
    connection
        .create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            window,
            screen_info.root,
            20,
            20,
            WIDTH,
            HEIGHT,
            0,
            WindowClass::INPUT_OUTPUT,
            screen_info.root_visual,
            &CreateWindowAux::new()
                .background_pixel(0x00f3f4f6)
                .event_mask(EventMask::EXPOSURE | EventMask::BUTTON_PRESS | EventMask::KEY_PRESS),
        )
        .map_err(x11_error)?
        .check()
        .map_err(x11_error)?;
    let identity = std::env::var("QA_FIXTURE_TITLE").unwrap_or_else(|_| "Target".into());
    let mut state = format!("{identity}:Ready");
    set_title(&connection, window, &state)?;
    let font = connection.generate_id().map_err(x11_error)?;
    connection
        .open_font(font, b"fixed")
        .map_err(x11_error)?
        .check()
        .map_err(x11_error)?;
    let gc = connection.generate_id().map_err(x11_error)?;
    connection
        .create_gc(
            gc,
            window,
            &CreateGCAux::new()
                .foreground(0x00111827)
                .background(0x00f3f4f6)
                .font(font),
        )
        .map_err(x11_error)?
        .check()
        .map_err(x11_error)?;
    connection
        .map_window(window)
        .map_err(x11_error)?
        .check()
        .map_err(x11_error)?;
    connection
        .set_input_focus(InputFocus::PARENT, window, x11rb::CURRENT_TIME)
        .map_err(x11_error)?
        .check()
        .map_err(x11_error)?;
    connection.flush().map_err(x11_error)?;
    loop {
        match connection.wait_for_event().map_err(x11_error)? {
            x11rb::protocol::Event::Expose(_) => {
                draw(&connection, window, gc, &state, hide_control)?
            }
            x11rb::protocol::Event::ButtonPress(event) => {
                if (20..220).contains(&event.event_x)
                    && (36..88).contains(&event.event_y)
                    && !ignore_click
                {
                    state = format!("{identity}:Clicked");
                    set_title(&connection, window, &state)?;
                    draw(&connection, window, gc, &state, hide_control)?;
                }
            }
            x11rb::protocol::Event::KeyPress(event) => {
                let keymap = connection
                    .get_keyboard_mapping(event.detail, 1)
                    .map_err(x11_error)?
                    .reply()
                    .map_err(x11_error)?;
                if let Some(keysym) = keymap.keysyms.first().copied()
                    && let Some(character) = char::from_u32(keysym).filter(char::is_ascii_lowercase)
                {
                    let previous = state
                        .split_once(":Typed:")
                        .map(|(_, value)| value)
                        .unwrap_or("");
                    state = format!("{identity}:Typed:{previous}{character}");
                    set_title(&connection, window, &state)?;
                    draw(&connection, window, gc, &state, hide_control)?;
                }
            }
            _ => {}
        }
    }
}

fn set_title(connection: &RustConnection, window: u32, value: &str) -> Result<(), String> {
    connection
        .change_property8(
            PropMode::REPLACE,
            window,
            AtomEnum::WM_NAME,
            AtomEnum::STRING,
            value.as_bytes(),
        )
        .map_err(x11_error)?
        .check()
        .map_err(x11_error)
}

fn draw(
    connection: &RustConnection,
    window: u32,
    gc: u32,
    state: &str,
    hide_control: bool,
) -> Result<(), String> {
    connection
        .clear_area(false, window, 0, 0, WIDTH, HEIGHT)
        .map_err(x11_error)?
        .check()
        .map_err(x11_error)?;
    connection
        .image_text8(window, gc, 24, 32, b"Deterministic desktop fixture")
        .map_err(x11_error)?
        .check()
        .map_err(x11_error)?;
    if !hide_control {
        connection
            .poly_rectangle(
                window,
                gc,
                &[Rectangle {
                    x: 20,
                    y: 36,
                    width: 200,
                    height: 52,
                }],
            )
            .map_err(x11_error)?
            .check()
            .map_err(x11_error)?;
    }
    if !hide_control {
        connection
            .image_text8(window, gc, 35, 68, b"CLICK")
            .map_err(x11_error)?
            .check()
            .map_err(x11_error)?;
    }
    connection
        .image_text8(window, gc, 24, 130, state.as_bytes())
        .map_err(x11_error)?
        .check()
        .map_err(x11_error)?;
    connection.flush().map_err(x11_error)
}

fn x11_error(error: impl std::fmt::Display) -> String {
    format!("desktop fixture X11 operation failed: {error}")
}
