use std::{collections::BTreeSet, thread, time::Duration};

use x11rb::protocol::{
    xproto::{ConnectionExt as _, InputFocus},
    xtest::ConnectionExt as _,
};

use crate::runtime_qa::drivers::desktop::display::{DesktopDisplay, WindowInfo};

use super::ActionResult;

pub(super) fn click(
    display: &DesktopDisplay,
    window_id: &str,
    x: u16,
    y: u16,
    double: bool,
    owner_pids: &BTreeSet<u32>,
) -> Result<ActionResult, String> {
    let window = super::window::require(display, window_id, owner_pids)?;
    let rect = display.window(window, owner_pids)?;
    let point = absolute_point(&rect, x, y)?;
    // The private Xvfb has no window manager to focus a clicked application.
    display
        .connection
        .set_input_focus(InputFocus::PARENT, window, x11rb::CURRENT_TIME)
        .map_err(super::x_error)?
        .check()
        .map_err(super::x_error)?;
    let count = if double { 2 } else { 1 };
    input(display, 6, 0, point)?;
    for _ in 0..count {
        input(display, 4, 1, point)?;
        input(display, 5, 1, point)?;
        if double {
            thread::sleep(Duration::from_millis(80));
        }
    }
    Ok(ActionResult::State(format!(
        "{}clicked window {window} at {x},{y}",
        if double { "double-" } else { "" }
    )))
}

pub(super) fn drag(
    display: &DesktopDisplay,
    window_id: &str,
    start: (u16, u16),
    end: (u16, u16),
    owner_pids: &BTreeSet<u32>,
) -> Result<ActionResult, String> {
    let window = super::window::require(display, window_id, owner_pids)?;
    let rect = display.window(window, owner_pids)?;
    let start = absolute_point(&rect, start.0, start.1)?;
    let end = absolute_point(&rect, end.0, end.1)?;
    // XTest button events use the current pointer position; move before press.
    input(display, 6, 0, start)?;
    input(display, 4, 1, start)?;
    input(display, 6, 0, end)?;
    input(display, 5, 1, end)?;
    Ok(ActionResult::State(format!("dragged in window {window}")))
}

pub(super) fn scroll(
    display: &DesktopDisplay,
    window_id: &str,
    delta_x: i16,
    delta_y: i16,
    owner_pids: &BTreeSet<u32>,
) -> Result<ActionResult, String> {
    if delta_x != 0 {
        return Err("horizontal scrolling is not supported by this X11 driver".into());
    }
    let window = super::window::require(display, window_id, owner_pids)?;
    let rect = display.window(window, owner_pids)?;
    let point = absolute_point(&rect, rect.width / 2, rect.height / 2)?;
    input(display, 6, 0, point)?;
    let button = if delta_y < 0 { 4 } else { 5 };
    for _ in 0..delta_y.unsigned_abs().min(40) {
        input(display, 4, button, point)?;
        input(display, 5, button, point)?;
    }
    Ok(ActionResult::State(format!(
        "scrolled window {window} by {delta_y}"
    )))
}

pub(super) fn type_character(display: &DesktopDisplay, character: char) -> Result<(), String> {
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

pub(super) fn chord(display: &DesktopDisplay, keys: &[String]) -> Result<(), String> {
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

fn input(display: &DesktopDisplay, event: u8, detail: u8, point: (i16, i16)) -> Result<(), String> {
    display
        .connection
        .xtest_fake_input(event, detail, 0, display.root, point.0, point.1, 0)
        .map_err(super::x_error)?
        .check()
        .map_err(super::x_error)
}

fn absolute_point(window: &WindowInfo, x: u16, y: u16) -> Result<(i16, i16), String> {
    if x >= window.width || y >= window.height {
        return Err("input coordinate is outside the target window".into());
    }
    let x = window.x.saturating_add(x);
    let y = window.y.saturating_add(y);
    if x >= super::super::display::DISPLAY_WIDTH || y >= super::super::display::DISPLAY_HEIGHT {
        return Err("input coordinate is outside the QA display".into());
    }
    Ok((x as i16, y as i16))
}
