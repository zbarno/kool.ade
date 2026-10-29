use std::collections::BTreeSet;
use std::{thread, time::Duration};

use x11rb::protocol::xproto::{ConfigureWindowAux, ConnectionExt as _, InputFocus};

use crate::runtime_qa::{QaDriverActionV1, QaScenarioStepV1};

use super::display::DesktopDisplay;

pub(super) mod pointer;
pub(super) mod window;

pub(super) enum ActionResult {
    State(String),
    Screenshot,
    Neutral,
}

pub(super) fn execute(
    display: &DesktopDisplay,
    step: &QaScenarioStepV1,
    wait_limit: Duration,
    owner_pids: &BTreeSet<u32>,
) -> Result<ActionResult, String> {
    match &step.driver_action {
        QaDriverActionV1::LaunchValidatedTarget => Ok(ActionResult::Neutral),
        QaDriverActionV1::WaitForWindow => {
            let deadline = std::time::Instant::now() + wait_limit.min(Duration::from_secs(10));
            loop {
                let windows = display.windows(owner_pids)?;
                let observed = window::describe(&windows);
                let expectation_met = step
                    .expected_state
                    .as_deref()
                    .is_none_or(|expected| super::run::expected_state_met(expected, &observed));
                if !windows.is_empty() && expectation_met {
                    return Ok(ActionResult::State(observed));
                }
                if std::time::Instant::now() >= deadline {
                    return Err(if step.expected_state.is_some() {
                        "timed out waiting for the expected application window state".into()
                    } else {
                        "timed out waiting for a visible application window".into()
                    });
                }
                thread::sleep(Duration::from_millis(50));
            }
        }
        QaDriverActionV1::ListWindows => Ok(ActionResult::State(window::describe(
            &display.windows(owner_pids)?,
        ))),
        QaDriverActionV1::AssertRegionVisible {
            window_id,
            x,
            y,
            width,
            height,
        } => {
            let id = window::require(display, window_id, owner_pids)?;
            let visible = display.region_has_visible_content(id, *x, *y, *width, *height)?;
            Ok(ActionResult::State(if visible {
                format!("window {id}: region visible")
            } else {
                format!("window {id}: region not visible")
            }))
        }
        QaDriverActionV1::FocusWindow { window_id } => {
            let window = window::require(display, window_id, owner_pids)?;
            display
                .connection
                .set_input_focus(InputFocus::PARENT, window, x11rb::CURRENT_TIME)
                .map_err(x_error)?
                .check()
                .map_err(x_error)?;
            Ok(ActionResult::State(format!("focused window {window}")))
        }
        QaDriverActionV1::ResizeWindow {
            window_id,
            width,
            height,
        } => {
            let window = window::require(display, window_id, owner_pids)?;
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
                "resized window {window} to {width}x{height}"
            )))
        }
        QaDriverActionV1::Click { window_id, x, y } => {
            pointer::click(display, window_id, *x, *y, false, owner_pids)
        }
        QaDriverActionV1::DoubleClick { window_id, x, y } => {
            pointer::click(display, window_id, *x, *y, true, owner_pids)
        }
        QaDriverActionV1::Drag {
            window_id,
            start_x,
            start_y,
            end_x,
            end_y,
        } => pointer::drag(
            display,
            window_id,
            (*start_x, *start_y),
            (*end_x, *end_y),
            owner_pids,
        ),
        QaDriverActionV1::TypeText { text } => {
            for character in text.chars() {
                pointer::type_character(display, character)?;
            }
            Ok(ActionResult::State(format!(
                "typed {} characters",
                text.chars().count()
            )))
        }
        QaDriverActionV1::KeyChord { keys } => {
            pointer::chord(display, keys)?;
            Ok(ActionResult::State(format!(
                "sent {}-key chord",
                keys.len()
            )))
        }
        QaDriverActionV1::Scroll {
            window_id,
            delta_x,
            delta_y,
        } => pointer::scroll(display, window_id, *delta_x, *delta_y, owner_pids),
        QaDriverActionV1::Screenshot => Ok(ActionResult::Screenshot),
        QaDriverActionV1::ReadAppLogs => {
            Ok(ActionResult::State("application logs requested".into()))
        }
        QaDriverActionV1::CloseWindow { window_id } => {
            let window = window::require(display, window_id, owner_pids)?;
            display
                .connection
                .destroy_window(window)
                .map_err(x_error)?
                .check()
                .map_err(x_error)?;
            Ok(ActionResult::State(format!("closed window {window}")))
        }
        _ => Err("scenario contains an action unsupported by the Linux desktop driver".into()),
    }
}

fn x_error(error: impl std::fmt::Display) -> String {
    format!("X11 desktop action failed: {error}")
}
