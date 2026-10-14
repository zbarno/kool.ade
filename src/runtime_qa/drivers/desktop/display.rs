use std::{
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use image::ImageEncoder;
use x11rb::{
    connection::Connection,
    image::Image,
    protocol::xproto::{AtomEnum, ConnectionExt, Window, WindowClass},
    rust_connection::RustConnection,
};

mod ownership;
mod visibility;

pub(super) const DISPLAY_NAME: &str = ":99";
pub(super) const DISPLAY_WIDTH: u16 = 1440;
pub(super) const DISPLAY_HEIGHT: u16 = 900;

pub(super) struct DesktopDisplay {
    server: Child,
    pub(super) connection: RustConnection,
    pub(super) root: Window,
    screen: usize,
    name_atom: u32,
    utf8_atom: u32,
}

#[derive(Clone, Debug)]
pub(super) struct WindowInfo {
    pub(super) id: Window,
    pub(super) x: u16,
    pub(super) y: u16,
    pub(super) width: u16,
    pub(super) height: u16,
    pub(super) title: String,
}

impl DesktopDisplay {
    pub(super) fn start() -> Result<Self, String> {
        let metadata = std::fs::symlink_metadata("/usr/bin/Xvfb")
            .map_err(|_| "pinned QA image does not provide Xvfb".to_owned())?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err("pinned QA image Xvfb executable is unsafe".into());
        }
        let mut server = Command::new("/usr/bin/Xvfb")
            .args([
                DISPLAY_NAME,
                "-screen",
                "0",
                "1440x900x24",
                "-nolisten",
                "tcp",
                "-noreset",
                "-ac",
            ])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", "/tmp")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("could not start private Xvfb: {error}"))?;
        let deadline = Instant::now() + Duration::from_secs(8);
        let (connection, screen) = loop {
            if server
                .try_wait()
                .map_err(|error| format!("could not inspect Xvfb: {error}"))?
                .is_some()
            {
                return Err("private Xvfb exited before becoming ready".into());
            }
            match RustConnection::connect(Some(DISPLAY_NAME)) {
                Ok(connected) => break connected,
                Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
                Err(_) => return Err("private Xvfb did not accept a local connection".into()),
            }
        };
        let root = connection.setup().roots[screen].root;
        let name_atom = connection
            .intern_atom(false, b"_NET_WM_NAME")
            .map_err(display_error)?
            .reply()
            .map_err(display_error)?
            .atom;
        let utf8_atom = connection
            .intern_atom(false, b"UTF8_STRING")
            .map_err(display_error)?
            .reply()
            .map_err(display_error)?
            .atom;
        Ok(Self {
            server,
            connection,
            root,
            screen,
            name_atom,
            utf8_atom,
        })
    }

    pub(super) fn windows(
        &self,
        owner_pids: &std::collections::BTreeSet<u32>,
    ) -> Result<Vec<WindowInfo>, String> {
        let tree = self
            .connection
            .query_tree(self.root)
            .map_err(display_error)?
            .reply()
            .map_err(display_error)?;
        let mut windows = Vec::new();
        for id in tree.children {
            let Ok(attributes) = self
                .connection
                .get_window_attributes(id)
                .map_err(display_error)?
                .reply()
            else {
                continue;
            };
            if attributes.map_state != x11rb::protocol::xproto::MapState::VIEWABLE
                || attributes.class == WindowClass::INPUT_ONLY
            {
                continue;
            }
            let Ok(geometry) = self
                .connection
                .get_geometry(id)
                .map_err(display_error)?
                .reply()
            else {
                continue;
            };
            if geometry.width == 0 || geometry.height == 0 {
                continue;
            }
            let position = self
                .connection
                .translate_coordinates(id, self.root, 0, 0)
                .map_err(display_error)?
                .reply()
                .map_err(display_error)?;
            let title = self.window_title(id).unwrap_or_default();
            let pid = ownership::window_client_pid(&self.connection, id)?;
            if !pid.is_some_and(|pid| owner_pids.contains(&pid)) {
                continue;
            }
            windows.push(WindowInfo {
                id,
                x: position.dst_x.max(0) as u16,
                y: position.dst_y.max(0) as u16,
                width: geometry.width,
                height: geometry.height,
                title,
            });
        }
        Ok(windows)
    }

    pub(super) fn window(
        &self,
        id: Window,
        owner_pids: &std::collections::BTreeSet<u32>,
    ) -> Result<WindowInfo, String> {
        self.windows(owner_pids)?
            .into_iter()
            .find(|window| window.id == id)
            .ok_or_else(|| "target window is no longer visible".into())
    }

    pub(super) fn screenshot_png(&self) -> Result<Vec<u8>, String> {
        let (image, visual_id) = Image::get(
            &self.connection,
            self.root,
            0,
            0,
            DISPLAY_WIDTH,
            DISPLAY_HEIGHT,
        )
        .map_err(display_error)?;
        let setup = self.connection.setup();
        let visual = setup.roots[self.screen]
            .allowed_depths
            .iter()
            .flat_map(|depth| &depth.visuals)
            .find(|visual| visual.visual_id == visual_id)
            .ok_or_else(|| "Xvfb returned an unknown screenshot visual".to_owned())?;
        let mut pixels =
            Vec::with_capacity(usize::from(DISPLAY_WIDTH) * usize::from(DISPLAY_HEIGHT) * 4);
        for y in 0..DISPLAY_HEIGHT {
            for x in 0..DISPLAY_WIDTH {
                let pixel = image.get_pixel(x, y);
                pixels.push(channel(pixel, visual.red_mask));
                pixels.push(channel(pixel, visual.green_mask));
                pixels.push(channel(pixel, visual.blue_mask));
                pixels.push(255);
            }
        }
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(
                &pixels,
                u32::from(DISPLAY_WIDTH),
                u32::from(DISPLAY_HEIGHT),
                image::ExtendedColorType::Rgba8,
            )
            .map_err(|error| format!("could not encode private display screenshot: {error}"))?;
        Ok(png)
    }

    pub(super) fn region_has_visible_content(
        &self,
        window: Window,
        x: u16,
        y: u16,
        width: u16,
        height: u16,
    ) -> Result<bool, String> {
        visibility::region_has_visible_content(&self.connection, window, x, y, width, height)
    }

    pub(super) fn keyboard_mapping(&self) -> Result<(u8, Vec<u32>, usize), String> {
        let setup = self.connection.setup();
        let first = setup.min_keycode;
        let count = setup.max_keycode - first + 1;
        let reply = self
            .connection
            .get_keyboard_mapping(first, count)
            .map_err(display_error)?
            .reply()
            .map_err(display_error)?;
        Ok((first, reply.keysyms, usize::from(reply.keysyms_per_keycode)))
    }

    pub(super) fn keycode(&self, keysym: u32) -> Option<u8> {
        let (first, symbols, per_keycode) = self.keyboard_mapping().ok()?;
        symbols
            .chunks(per_keycode)
            .position(|keysyms| keysyms.contains(&keysym))
            .and_then(|offset| u8::try_from(usize::from(first) + offset).ok())
    }

    fn window_title(&self, window: Window) -> Result<String, String> {
        let value = self
            .connection
            .get_property(false, window, self.name_atom, self.utf8_atom, 0, 1024)
            .map_err(display_error)?
            .reply()
            .map_err(display_error)?;
        if !value.value.is_empty() {
            return Ok(sanitize_title(&String::from_utf8_lossy(&value.value)));
        }
        let value = self
            .connection
            .get_property(false, window, AtomEnum::WM_NAME, AtomEnum::STRING, 0, 1024)
            .map_err(display_error)?
            .reply()
            .map_err(display_error)?;
        Ok(sanitize_title(&String::from_utf8_lossy(&value.value)))
    }
}

impl Drop for DesktopDisplay {
    fn drop(&mut self) {
        let _ = self.server.kill();
        let _ = self.server.wait();
    }
}

fn channel(pixel: u32, mask: u32) -> u8 {
    if mask == 0 {
        return 0;
    }
    let shift = mask.trailing_zeros();
    let max = mask >> shift;
    (((pixel & mask) >> shift) * 255 / max) as u8
}

fn sanitize_title(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(512)
        .collect()
}

fn display_error(error: impl std::fmt::Display) -> String {
    format!("private X11 display operation failed: {error}")
}
