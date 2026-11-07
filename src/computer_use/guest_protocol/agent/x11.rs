//! Fixed guest-local X11 primitives; this module has no host path or shell API.

mod capture;
mod input;

pub(in crate::computer_use::guest_protocol::agent) use capture::capture_png;
pub(in crate::computer_use::guest_protocol::agent) use input::{click, key_press, type_ascii};
