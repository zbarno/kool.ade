//! Fixed guest-local X11 primitives; this module has no host path or shell API.

use x11rb::{connection::Connection, protocol::xproto::ConnectionExt as _};

mod capture;
mod input;
mod windows;

pub(in crate::computer_use::guest_protocol::agent) use capture::capture_png;
pub(in crate::computer_use::guest_protocol::agent) use input::{
    click, double_click, drag, focus_window, key_chord, key_press, move_pointer, scroll, type_text,
};
pub(in crate::computer_use::guest_protocol::agent) use windows::resolve_window;

pub(super) fn capabilities() -> (bool, bool) {
    let Ok((connection, screen_index)) = input::connect() else {
        return (false, false);
    };
    let root = connection.setup().roots[screen_index].root;
    let screen = connection
        .get_geometry(root)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .is_some_and(|geometry| geometry.width > 0 && geometry.height > 0);
    let input = connection
        .query_extension(b"XTEST")
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .is_some_and(|extension| extension.present);
    (screen, input)
}
