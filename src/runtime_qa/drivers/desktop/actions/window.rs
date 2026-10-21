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
    resolve(raw, &display.windows(owner_pids)?)
}

fn resolve(raw: &str, windows: &[WindowInfo]) -> Result<Window, String> {
    if raw == "active" {
        return match windows {
            [window] => Ok(window.id),
            [] => Err("the approved target has no visible window".into()),
            _ => Err("the approved target has multiple visible windows; select a window ID".into()),
        };
    }
    let id = raw
        .parse::<Window>()
        .map_err(|_| "window ID is invalid".to_owned())?;
    if windows.iter().any(|window| window.id == id) {
        Ok(id)
    } else {
        Err("window ID is not a currently visible approved target window".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(id: Window) -> WindowInfo {
        WindowInfo {
            id,
            x: 0,
            y: 0,
            width: 640,
            height: 480,
            title: "fixture".into(),
        }
    }

    #[test]
    fn active_resolves_only_when_one_owned_window_is_visible() {
        assert_eq!(resolve("active", &[window(7)]).unwrap(), 7);
        assert!(
            resolve("active", &[])
                .unwrap_err()
                .contains("no visible window")
        );
        assert!(
            resolve("active", &[window(7), window(8)])
                .unwrap_err()
                .contains("multiple visible windows")
        );
    }

    #[test]
    fn numeric_window_ids_still_require_a_current_owned_window() {
        assert_eq!(resolve("7", &[window(7)]).unwrap(), 7);
        assert!(resolve("8", &[window(7)]).is_err());
        assert!(resolve("not-a-window", &[window(7)]).is_err());
    }
}
