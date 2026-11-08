use super::super::{AgentReplayState, x11};
use crate::computer_use::{
    ComputerActionV1, ComputerFrameReferenceV1, ComputerWaitConditionV1,
    guest_protocol::{
        GuestActionOutcomeV1, GuestActionReplyV1, GuestBootConfigV1, GuestObservationV1,
        GuestOperationResultV1 as ResultStatus, GuestProtocolErrorV1,
    },
};
use std::time::{Duration, Instant};

pub(super) fn complete_with_observation(
    action: &ComputerActionV1,
    config: &GuestBootConfigV1,
    replay: &mut AgentReplayState,
) -> Result<GuestActionOutcomeV1, GuestProtocolErrorV1> {
    let observation = x11::capture_png(&config.session_id, replay.latest_observation.as_ref())?;
    replay.latest_observation = Some(observation.clone());
    Ok(GuestActionOutcomeV1 {
        reply: GuestActionReplyV1 {
            action_sequence: action.action_sequence,
            result: ResultStatus::Completed,
        },
        observation: Some(observation),
    })
}

pub(super) fn wait_for_app_window(
    action: &ComputerActionV1,
    app_id: &str,
    config: &GuestBootConfigV1,
    replay: &mut AgentReplayState,
) -> Result<GuestActionOutcomeV1, GuestProtocolErrorV1> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let mut outcome = complete_with_observation(action, config, replay)?;
        if outcome.observation.as_ref().is_some_and(|observation| {
            observation
                .windows
                .iter()
                .any(|window| window.app_id.as_deref() == Some(app_id))
        }) {
            return Ok(outcome);
        }
        if Instant::now() >= deadline {
            outcome.reply.result = ResultStatus::TimedOut;
            outcome.observation = None;
            return Ok(outcome);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

pub(super) fn refresh_and_validate_frame(
    config: &GuestBootConfigV1,
    target: &ComputerFrameReferenceV1,
    replay: &mut AgentReplayState,
) -> Result<bool, GuestProtocolErrorV1> {
    let Some(previous) = replay.latest_observation.as_ref() else {
        return Ok(false);
    };
    if !previous.matches_frame_reference(target) {
        return Ok(false);
    }
    let fresh = x11::capture_png(&config.session_id, Some(previous))?;
    let stable = previous.width_px == fresh.width_px
        && previous.height_px == fresh.height_px
        && previous.display_generation == fresh.display_generation
        && previous.screenshot_sha256 == fresh.screenshot_sha256
        && same_windows(previous, &fresh);
    replay.latest_observation = Some(fresh);
    Ok(stable)
}

pub(super) fn refresh_and_validate_window(
    window_id: &str,
    revision: u64,
    config: &GuestBootConfigV1,
    replay: &mut AgentReplayState,
) -> Result<bool, GuestProtocolErrorV1> {
    let Some(previous) = replay.latest_observation.as_ref() else {
        return Ok(false);
    };
    if previous.observation_revision != revision
        || !previous
            .windows
            .iter()
            .any(|window| window.window_id == window_id)
    {
        return Ok(false);
    }
    let fresh = x11::capture_png(&config.session_id, Some(previous))?;
    let stable = previous.width_px == fresh.width_px
        && previous.height_px == fresh.height_px
        && previous.display_generation == fresh.display_generation
        && same_windows(previous, &fresh);
    replay.latest_observation = Some(fresh);
    Ok(stable)
}

pub(super) fn refresh_and_validate_focused_window(
    window_id: &str,
    revision: u64,
    config: &GuestBootConfigV1,
    replay: &mut AgentReplayState,
) -> Result<bool, GuestProtocolErrorV1> {
    if !refresh_and_validate_window(window_id, revision, config, replay)? {
        return Ok(false);
    }
    Ok(replay
        .latest_observation
        .as_ref()
        .is_some_and(|observation| {
            observation
                .windows
                .iter()
                .any(|window| window.window_id == window_id && window.focused)
        }))
}

fn same_windows(previous: &GuestObservationV1, current: &GuestObservationV1) -> bool {
    previous.windows == current.windows
}

pub(super) fn wait_for(
    condition: &ComputerWaitConditionV1,
    timeout_ms: u32,
    config: &GuestBootConfigV1,
    replay: &mut AgentReplayState,
) -> Result<bool, GuestProtocolErrorV1> {
    let deadline = Instant::now() + Duration::from_millis(u64::from(timeout_ms));
    loop {
        let observation = x11::capture_png(&config.session_id, replay.latest_observation.as_ref())?;
        replay.latest_observation = Some(observation.clone());
        if condition_matches(condition, &observation) {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        std::thread::sleep(
            Duration::from_millis(100).min(deadline.saturating_duration_since(Instant::now())),
        );
    }
}

fn condition_matches(
    condition: &ComputerWaitConditionV1,
    observation: &GuestObservationV1,
) -> bool {
    match condition {
        ComputerWaitConditionV1::ScreenDiffers { screenshot_sha256 } => {
            observation.screenshot_sha256 != *screenshot_sha256
        }
        ComputerWaitConditionV1::WindowTitleContains { text } => observation
            .windows
            .iter()
            .any(|window| window.title.contains(text)),
        ComputerWaitConditionV1::WindowVisible { window_id } => observation
            .windows
            .iter()
            .any(|window| &window.window_id == window_id),
        ComputerWaitConditionV1::WindowClosed { window_id } => observation
            .windows
            .iter()
            .all(|window| &window.window_id != window_id),
    }
}
