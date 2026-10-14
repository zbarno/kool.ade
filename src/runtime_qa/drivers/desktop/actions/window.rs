use std::collections::BTreeSet;

use x11rb::protocol::xproto::Window;

use super::super::display::{DesktopDisplay, WindowInfo};

pub(in crate::runtime_qa::drivers::desktop) fn describe(windows: &[WindowInfo]) -> String {
    if windows.is_empty() {
        return "no visible windows".into();
    }
    windows
        .iter()
        .map(|window| {
            format!(
                "{}:{}:{}x{}@{},{}",
                window.id, window.title, window.width, window.height, window.x, window.y
            )
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

pub(in crate::runtime_qa::drivers::desktop) fn require(
    display: &DesktopDisplay,
    raw: &str,
    owner_pids: &BTreeSet<u32>,
) -> Result<Window, String> {
    let id = raw
        .parse::<Window>()
        .map_err(|_| "window ID is invalid".to_owned())?;
    if display
        .windows(owner_pids)?
        .iter()
        .any(|window| window.id == id)
    {
        Ok(id)
    } else {
        Err("window ID is not a currently visible approved target window".into())
    }
}
