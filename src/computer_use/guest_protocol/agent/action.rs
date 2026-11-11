mod observation;

use super::{AgentReplayState, x11};
use crate::computer_use::{
    ComputerActionPayloadV1 as Payload, ComputerActionV1,
    guest_protocol::{
        GuestActionOutcomeV1, GuestActionReplyV1, GuestBootConfigV1,
        GuestOperationResultV1 as ResultStatus, GuestProtocolErrorV1,
    },
};
use observation::{
    complete_with_observation, refresh_and_validate_focused_window, refresh_and_validate_frame,
    refresh_and_validate_window, wait_for, wait_for_app_window,
};

pub(super) fn execute_action(
    action: &ComputerActionV1,
    config: &GuestBootConfigV1,
    replay: &mut AgentReplayState,
) -> Result<GuestActionOutcomeV1, GuestProtocolErrorV1> {
    let result = match &action.payload {
        Payload::ObserveScreen | Payload::ObserveWindows | Payload::QueryState => {
            return complete_with_observation(action, config, replay);
        }
        Payload::ReadAccessibility => ResultStatus::UnsupportedCapability,
        Payload::MouseClick {
            target,
            x,
            y,
            button,
        } => {
            if !refresh_and_validate_frame(config, target, replay)? {
                ResultStatus::StaleObservation
            } else {
                x11::click(*x, *y, *button)?;
                return complete_with_observation(action, config, replay);
            }
        }
        Payload::MouseDoubleClick {
            target,
            x,
            y,
            button,
        } => {
            if !refresh_and_validate_frame(config, target, replay)? {
                ResultStatus::StaleObservation
            } else {
                x11::double_click(*x, *y, *button)?;
                return complete_with_observation(action, config, replay);
            }
        }
        Payload::MouseMove { target, x, y } => {
            if !refresh_and_validate_frame(config, target, replay)? {
                ResultStatus::StaleObservation
            } else {
                x11::move_pointer(*x, *y)?;
                return complete_with_observation(action, config, replay);
            }
        }
        Payload::MouseDrag {
            target,
            from_x,
            from_y,
            to_x,
            to_y,
            button,
        } => {
            if !refresh_and_validate_frame(config, target, replay)? {
                ResultStatus::StaleObservation
            } else {
                x11::drag(*from_x, *from_y, *to_x, *to_y, *button)?;
                return complete_with_observation(action, config, replay);
            }
        }
        Payload::MouseScroll {
            target,
            x,
            y,
            delta_x,
            delta_y,
        } => {
            if !refresh_and_validate_frame(config, target, replay)? {
                ResultStatus::StaleObservation
            } else {
                x11::scroll(*x, *y, *delta_x, *delta_y)?;
                return complete_with_observation(action, config, replay);
            }
        }
        Payload::FocusWindow {
            window_id,
            observation_revision,
        } => {
            if !refresh_and_validate_window(window_id, *observation_revision, config, replay)? {
                ResultStatus::StaleObservation
            } else {
                let current = replay
                    .latest_observation
                    .as_ref()
                    .ok_or(GuestProtocolErrorV1::StaleObservation)?;
                let xid = x11::resolve_window(&config.session_id, current, window_id)?;
                x11::focus_window(xid)?;
                let mut outcome = complete_with_observation(action, config, replay)?;
                if !outcome.observation.as_ref().is_some_and(|observation| {
                    observation
                        .windows
                        .iter()
                        .any(|window| window.window_id == *window_id && window.focused)
                }) {
                    outcome.reply.result = ResultStatus::FocusRequired;
                    outcome.observation = None;
                }
                return Ok(outcome);
            }
        }
        Payload::KeyPress {
            key,
            window_id,
            observation_revision,
        } => {
            if !refresh_and_validate_focused_window(
                window_id,
                *observation_revision,
                config,
                replay,
            )? {
                ResultStatus::FocusRequired
            } else {
                match key {
                    crate::computer_use::ComputerKeyV1::Named(key) => x11::key_press(*key)?,
                    crate::computer_use::ComputerKeyV1::Character(character) => {
                        x11::type_text(&character.to_string())?;
                    }
                }
                return complete_with_observation(action, config, replay);
            }
        }
        Payload::KeyChord {
            keys,
            window_id,
            observation_revision,
        } => {
            if !refresh_and_validate_focused_window(
                window_id,
                *observation_revision,
                config,
                replay,
            )? {
                ResultStatus::FocusRequired
            } else {
                x11::key_chord(keys)?;
                return complete_with_observation(action, config, replay);
            }
        }
        Payload::TypeText {
            text,
            window_id,
            observation_revision,
        } => {
            if !refresh_and_validate_focused_window(
                window_id,
                *observation_revision,
                config,
                replay,
            )? {
                ResultStatus::FocusRequired
            } else {
                x11::type_text(text)?;
                return complete_with_observation(action, config, replay);
            }
        }
        Payload::LaunchApp { app_id } => {
            if !config.allowed_apps.iter().any(|allowed| allowed == app_id)
                || platform::launch_allowed_app(app_id).is_err()
            {
                ResultStatus::UnsupportedCapability
            } else {
                return wait_for_app_window(action, app_id, config, replay);
            }
        }
        Payload::ManageApp { app_id, .. } => {
            if action.actor != crate::computer_use::ComputerActionActorV1::Human
                || !config.allowed_apps.iter().any(|allowed| allowed == app_id)
            {
                ResultStatus::UnsupportedCapability
            } else {
                match crate::computer_use::apps::guest_helper::manage(action) {
                    Ok(result) => result,
                    Err(GuestProtocolErrorV1::UnsupportedCapability) => {
                        ResultStatus::UnsupportedCapability
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        Payload::LaunchApprovedStagedTaskApp { application } => {
            if action.actor != crate::computer_use::ComputerActionActorV1::Human
                || !config
                    .allowed_apps
                    .iter()
                    .any(|allowed| allowed == &application.app_id)
            {
                ResultStatus::UnsupportedCapability
            } else {
                crate::computer_use::apps::guest_helper::launch_staged_task(action)?
            }
        }
        Payload::WaitFor {
            condition,
            timeout_ms,
        } => {
            if wait_for(condition, *timeout_ms, config, replay)? {
                return complete_with_observation(action, config, replay);
            }
            ResultStatus::TimedOut
        }
    };
    Ok(GuestActionOutcomeV1 {
        reply: GuestActionReplyV1 {
            action_sequence: action.action_sequence,
            result,
        },
        observation: None,
    })
}

use super::platform;
