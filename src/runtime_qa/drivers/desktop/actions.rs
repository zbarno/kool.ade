use std::{thread, time::Duration};

use x11rb::protocol::{
    xproto::{ConfigureWindowAux, ConnectionExt as _, InputFocus, Window},
    xtest::ConnectionExt as _,
};

use crate::runtime_qa::{QaDriverActionV1, QaScenarioStepV1};

use super::display::{DesktopDisplay, WindowInfo};

pub(super) enum ActionResult {
    State(String),
    Screenshot,
    Neutral,
}

pub(super) fn execute(
    display: &DesktopDisplay,
    step: &QaScenarioStepV1,
    wait_limit: Duration,
) -> Result<ActionResult, String> {
    match &step.driver_action {
        QaDriverActionV1::LaunchValidatedTarget => Ok(ActionResult::Neutral),
        QaDriverActionV1::WaitForWindow => {
            let deadline = std::time::Instant::now() + wait_limit.min(Duration::from_secs(10));
            loop {
                let windows = display.windows()?;
                if !windows.is_empty() {
                    return Ok(ActionResult::State(describe_windows(&windows)));
                }
                if std::time::Instant::now() >= deadline {
                    return Err("timed out waiting for a visible application window".into());
                }
                thread::sleep(Duration::from_millis(50));
            }
        }
        QaDriverActionV1::ListWindows => {
            Ok(ActionResult::State(describe_windows(&display.windows()?)))
        }
        QaDriverActionV1::FocusWindow { window_id } => {
            let window = require_window(display, window_id)?;
            display
                .connection
                .set_input_focus(InputFocus::PARENT, window, x11rb::CURRENT_TIME)
                .map_err(x_error)?
                .check()
                .map_err(x_error)?;
            Ok(ActionResult::State(format!("focused window {window_id}")))
        }
        QaDriverActionV1::ResizeWindow {
            window_id,
            width,
            height,
        } => {
            let window = require_window(display, window_id)?;
            if !(320..=super::display::DISPLAY_WIDTH).contains(width)
                || !(240..=super::display::DISPLAY_HEIGHT).contains(height)
            {
                return Err("requested window size exceeds the QA display".into());
            }
            display
                .connection
                .configure_window(
                    window,
                    &ConfigureWindowAux::new()
                        .width(u32::from(*width))
                        .height(u32::from(*height)),
                )
                .map_err(x_error)?
                .check()
                .map_err(x_error)?;
            Ok(ActionResult::State(format!(
                "resized window {window_id} to {width}x{height}"
            )))
        }
        QaDriverActionV1::Click { window_id, x, y } => click(display, window_id, *x, *y, false),
        QaDriverActionV1::DoubleClick { window_id, x, y } => {
            click(display, window_id, *x, *y, true)
        }
        QaDriverActionV1::Drag {
            window_id,
            start_x,
            start_y,
            end_x,
            end_y,
        } => {
            let window = require_window(display, window_id)?;
            let rect = display.window(window)?;
            let start = absolute_point(&rect, *start_x, *start_y)?;
            let end = absolute_point(&rect, *end_x, *end_y)?;
            input(display, 4, 1, start)?;
            input(display, 6, 0, start)?;
            input(display, 6, 0, end)?;
            input(display, 5, 1, end)?;
            Ok(ActionResult::State(format!(
                "dragged in window {window_id}"
            )))
        }
        QaDriverActionV1::TypeText { text } => {
            for character in text.chars() {
                type_character(display, character)?;
            }
            Ok(ActionResult::State(format!(
                "typed {} characters",
                text.chars().count()
            )))
        }
        QaDriverActionV1::KeyChord { keys } => {
            chord(display, keys)?;
            Ok(ActionResult::State(format!(
                "sent {}-key chord",
                keys.len()
            )))
        }
        QaDriverActionV1::Scroll {
            window_id,
            delta_x,
            delta_y,
        } => {
            if *delta_x != 0 {
                return Err("horizontal scrolling is not supported by this X11 driver".into());
            }
            let window = require_window(display, window_id)?;
            let rect = display.window(window)?;
            let point = absolute_point(&rect, rect.width / 2, rect.height / 2)?;
            let button = if *delta_y < 0 { 4 } else { 5 };
            for _ in 0..delta_y.unsigned_abs().min(40) {
                input(display, 4, button, point)?;
                input(display, 5, button, point)?;
            }
            Ok(ActionResult::State(format!(
                "scrolled window {window_id} by {delta_y}"
            )))
        }
        QaDriverActionV1::Screenshot => Ok(ActionResult::Screenshot),
        QaDriverActionV1::ReadAppLogs => {
            Ok(ActionResult::State("application logs requested".into()))
        }
        QaDriverActionV1::CloseWindow { window_id } => {
            let window = require_window(display, window_id)?;
            display
                .connection
                .destroy_window(window)
                .map_err(x_error)?
                .check()
                .map_err(x_error)?;
            Ok(ActionResult::State(format!("closed window {window_id}")))
        }
        _ => Err("scenario contains an action unsupported by the Linux desktop driver".into()),
    }
}

pub(super) fn describe_windows(windows: &[WindowInfo]) -> String {
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

fn click(
    display: &DesktopDisplay,
    window_id: &str,
    x: u16,
    y: u16,
    double: bool,
) -> Result<ActionResult, String> {
    let window = require_window(display, window_id)?;
    let rect = display.window(window)?;
    let point = absolute_point(&rect, x, y)?;
    let count = if double { 2 } else { 1 };
    for _ in 0..count {
        input(display, 4, 1, point)?;
        input(display, 5, 1, point)?;
        if double {
            thread::sleep(Duration::from_millis(80));
        }
    }
    Ok(ActionResult::State(format!(
        "{}clicked window {window_id} at {x},{y}",
        if double { "double-" } else { "" }
    )))
}

fn require_window(display: &DesktopDisplay, raw: &str) -> Result<Window, String> {
    let id = raw
        .parse::<Window>()
        .map_err(|_| "window ID is invalid".to_owned())?;
    if display.windows()?.iter().any(|window| window.id == id) {
        Ok(id)
    } else {
        Err("window ID is not a currently visible application window".into())
    }
}

fn input(display: &DesktopDisplay, event: u8, detail: u8, point: (i16, i16)) -> Result<(), String> {
    display
        .connection
        .xtest_fake_input(event, detail, 0, display.root, point.0, point.1, 0)
        .map_err(x_error)?
        .check()
        .map_err(x_error)
}

fn absolute_point(window: &WindowInfo, x: u16, y: u16) -> Result<(i16, i16), String> {
    if x >= window.width || y >= window.height {
        return Err("input coordinate is outside the target window".into());
    }
    let x = window.x.saturating_add(x);
    let y = window.y.saturating_add(y);
    if x >= super::display::DISPLAY_WIDTH || y >= super::display::DISPLAY_HEIGHT {
        return Err("input coordinate is outside the QA display".into());
    }
    Ok((x as i16, y as i16))
}

fn type_character(display: &DesktopDisplay, character: char) -> Result<(), String> {
    let (keysym, shifted) = match character {
        'a'..='z' => (character as u32, false),
        'A'..='Z' => (character.to_ascii_lowercase() as u32, true),
        '0'..='9' | ' ' => (character as u32, false),
        '\n' => (0xff0d, false),
        '\t' => (0xff09, false),
        _ => {
            return Err(
                "typed character is not supported by the deterministic US keyboard map".into(),
            );
        }
    };
    let keycode = display
        .keycode(keysym)
        .ok_or_else(|| "Xvfb keyboard map lacks a requested key".to_owned())?;
    if shifted {
        key(display, 2, 50)?;
    }
    key(display, 2, keycode)?;
    key(display, 3, keycode)?;
    if shifted {
        key(display, 3, 50)?;
    }
    Ok(())
}

fn chord(display: &DesktopDisplay, keys: &[String]) -> Result<(), String> {
    let mut codes = Vec::with_capacity(keys.len());
    for name in keys {
        let keysym = match name.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => 0xffe3,
            "shift" => 0xffe1,
            "alt" => 0xffe9,
            "enter" | "return" => 0xff0d,
            "escape" | "esc" => 0xff1b,
            "tab" => 0xff09,
            "backspace" => 0xff08,
            "delete" => 0xffff,
            value if value.len() == 1 => value.as_bytes()[0].to_ascii_lowercase() as u32,
            _ => return Err("key chord contains an unsupported key name".into()),
        };
        codes.push(
            display
                .keycode(keysym)
                .ok_or_else(|| "Xvfb keyboard map lacks a chord key".to_owned())?,
        );
    }
    for code in &codes {
        key(display, 2, *code)?;
    }
    for code in codes.iter().rev() {
        key(display, 3, *code)?;
    }
    Ok(())
}

fn key(display: &DesktopDisplay, event: u8, code: u8) -> Result<(), String> {
    input(display, event, code, (0, 0))
}

fn x_error(error: impl std::fmt::Display) -> String {
    format!("X11 desktop action failed: {error}")
}
