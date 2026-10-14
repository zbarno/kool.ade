use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sha2::Digest;

use crate::runtime_qa::{
    QaActionEvidenceV1, QaAssertionStatus, QaAssertionV1, QaDisplayV1, QaDriverActionV1,
    QaDriverOutputV1, QaDriverScreenshotV1, QaRunRequestV1, QaScenarioV1, QaValidatedTargetV1,
};

use super::{
    actions::{self, ActionResult},
    display::{DISPLAY_HEIGHT, DISPLAY_WIDTH, DesktopDisplay},
    process::TargetProcess,
};

mod expectation;
pub(super) use expectation::expected_state_met;

const DRIVER_VERSION: &str = "linux-x11-driver/v1";
pub(crate) fn run(
    request: &QaRunRequestV1,
    scenario: &QaScenarioV1,
    target: &QaValidatedTargetV1,
) -> Result<QaDriverOutputV1, String> {
    request.validate()?;
    scenario.validate(request)?;
    target.validate_for(request)?;
    if !request
        .relevant_platforms
        .contains(&crate::runtime_qa::QaPlatform::LinuxDesktop)
    {
        return Err("runtime QA request does not authorize Linux desktop execution".into());
    }
    let mut output = QaDriverOutputV1 {
        driver_version: DRIVER_VERSION.into(),
        completed: false,
        failure: None,
        display: QaDisplayV1 {
            width: DISPLAY_WIDTH,
            height: DISPLAY_HEIGHT,
            dpi: 96,
            theme: "default".into(),
            locale: "C.UTF-8".into(),
        },
        actions: Vec::new(),
        screenshots: Vec::new(),
        assertions: Vec::new(),
        logs: Vec::new(),
        crashes: Vec::new(),
        browser_console: Vec::new(),
        failed_requests: Vec::new(),
    };
    let deadline =
        Instant::now() + Duration::from_secs(u64::from(request.bounded_limits.timeout_seconds));
    let display = match DesktopDisplay::start() {
        Ok(display) => display,
        Err(error) => {
            output.failure = Some(error);
            return Ok(output);
        }
    };
    let mut process: Option<TargetProcess> = None;
    let mut screenshot_bytes = 0u64;
    for (index, step) in scenario.steps.iter().enumerate() {
        if Instant::now() >= deadline {
            output.failure = Some("runtime QA scenario exceeded its approved timeout".into());
            break;
        }
        let command = action_name(&step.driver_action);
        let owner_pids = process
            .as_ref()
            .map(TargetProcess::owned_pids)
            .unwrap_or_default();
        let before = actions::window::describe(&display.windows(&owner_pids).unwrap_or_default());
        if matches!(step.driver_action, QaDriverActionV1::LaunchValidatedTarget) {
            if process.is_some() {
                output.failure =
                    Some("validated target may only be launched once per scenario".into());
                break;
            }
            match TargetProcess::launch(request, target) {
                Ok(child) => process = Some(child),
                Err(error) => {
                    output.failure = Some(error);
                    break;
                }
            }
        } else if process.is_none() {
            output.failure =
                Some("scenario must launch its approved target before interacting".into());
            break;
        }
        if matches!(step.driver_action, QaDriverActionV1::ReadAppLogs) {
            output.logs = process
                .as_ref()
                .map(TargetProcess::logs)
                .unwrap_or_default();
        }
        let result = actions::execute(
            &display,
            step,
            deadline.saturating_duration_since(Instant::now()),
            &process
                .as_ref()
                .map(TargetProcess::owned_pids)
                .unwrap_or_default(),
        );
        let (post_state, screenshot_requested) = match result {
            Ok(ActionResult::State(value)) => (Some(value), false),
            Ok(ActionResult::Screenshot) => (Some(before.clone()), true),
            Ok(ActionResult::Neutral) => (Some(before.clone()), false),
            Err(error) => {
                output.failure = Some(error);
                break;
            }
        };
        let expected_state_mismatch = step.expected_state.as_ref().is_some_and(|expected| {
            !post_state
                .as_deref()
                .is_some_and(|observed| expected_state_met(expected, observed))
        });
        let should_screenshot =
            screenshot_requested || step.screenshot_checkpoint || expected_state_mismatch;
        let evidence_id = if should_screenshot {
            if output.screenshots.len() >= usize::from(request.bounded_limits.max_screenshots) {
                output.failure =
                    Some("runtime QA scenario exceeded the approved screenshot count".into());
                break;
            }
            // Give the target a bounded paint interval so evidence reflects
            // the state after an action instead of racing the next frame.
            std::thread::sleep(Duration::from_millis(120));
            match display.screenshot_png() {
                Ok(bytes) => {
                    screenshot_bytes = screenshot_bytes.saturating_add(bytes.len() as u64);
                    if screenshot_bytes > request.bounded_limits.max_evidence_bytes {
                        output.failure = Some(
                            "runtime QA screenshots exceeded the approved evidence byte limit"
                                .into(),
                        );
                        break;
                    }
                    let id = uuid::Uuid::new_v4().hyphenated().to_string();
                    let sha256 = format!("{:x}", sha2::Sha256::digest(&bytes));
                    output.screenshots.push(QaDriverScreenshotV1 {
                        id: id.clone(),
                        scenario_id: scenario.id.clone(),
                        sha256,
                        width: DISPLAY_WIDTH,
                        height: DISPLAY_HEIGHT,
                        source_action: index as u32,
                        redaction_masks: Vec::new(),
                        bytes,
                    });
                    Some(id)
                }
                Err(error) => {
                    output.failure = Some(error);
                    break;
                }
            }
        } else {
            None
        };
        let mut evidence_ids = evidence_id.into_iter().collect::<Vec<_>>();
        let occurred_at_unix_ms = now_ms();
        output.actions.push(QaActionEvidenceV1 {
            index: index as u32,
            scenario_id: scenario.id.clone(),
            command: command.into(),
            args_redacted: redacted_arguments(&step.driver_action),
            pre_state: Some(before),
            post_state: post_state.clone(),
            evidence_ids: evidence_ids.clone(),
            occurred_at_unix_ms,
        });
        if let (Some(expected), Some(observed)) = (&step.expected_state, post_state) {
            let passed = expected_state_met(expected, &observed);
            output.assertions.push(QaAssertionV1 {
                id: uuid::Uuid::new_v4().hyphenated().to_string(),
                scenario_ids: vec![scenario.id.clone()],
                requirement_ids: scenario.requirement_ids.clone(),
                expected: expected.clone(),
                observed,
                // An observed mismatch is a functional failure. Reserve
                // Untested for expectations the driver could not observe.
                status: if passed {
                    QaAssertionStatus::Passed
                } else {
                    QaAssertionStatus::Failed
                },
                evidence_refs: std::mem::take(&mut evidence_ids),
            });
        }
        if index + 1 < scenario.steps.len()
            && process
                .as_mut()
                .is_some_and(|child| child.is_running().ok() == Some(false))
        {
            output
                .crashes
                .push("approved target exited during runtime QA".into());
            output.failure =
                Some("approved application exited before the scenario completed".into());
            break;
        }
    }
    if output.failure.is_none() {
        output.logs = process
            .as_ref()
            .map(TargetProcess::logs)
            .unwrap_or_default();
        output.completed = output.actions.len() == scenario.steps.len();
    }
    if output.assertions.is_empty() {
        for (index, requirement) in scenario.requirement_ids.iter().enumerate() {
            let Some(screenshot) = output.screenshots.last() else {
                break;
            };
            output.assertions.push(QaAssertionV1 {
                id: uuid::Uuid::new_v4().hyphenated().to_string(),
                scenario_ids: vec![scenario.id.clone()],
                requirement_ids: vec![requirement.clone()],
                expected: request.acceptance_criteria.get(index).cloned().unwrap_or_else(|| scenario.expected_results[0].clone()),
                observed: "No deterministic assertion matched the criterion; screenshot retained for independent review".into(),
                status: QaAssertionStatus::Untested,
                evidence_refs: vec![screenshot.id.clone()],
            });
        }
    }
    Ok(output)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn action_name(action: &QaDriverActionV1) -> &'static str {
    match action {
        QaDriverActionV1::LaunchValidatedTarget => "launch_validated_target",
        QaDriverActionV1::WaitForWindow => "wait_for_window",
        QaDriverActionV1::ListWindows => "list_windows",
        QaDriverActionV1::AssertRegionVisible { .. } => "assert_region_visible",
        QaDriverActionV1::FocusWindow { .. } => "focus_window",
        QaDriverActionV1::ResizeWindow { .. } => "resize_window",
        QaDriverActionV1::Click { .. } => "click",
        QaDriverActionV1::DoubleClick { .. } => "double_click",
        QaDriverActionV1::Drag { .. } => "drag",
        QaDriverActionV1::TypeText { .. } => "type_text",
        QaDriverActionV1::KeyChord { .. } => "key_chord",
        QaDriverActionV1::Scroll { .. } => "scroll",
        QaDriverActionV1::Screenshot => "screenshot",
        QaDriverActionV1::ReadAppLogs => "read_app_logs",
        QaDriverActionV1::CloseWindow { .. } => "close",
        _ => "unsupported",
    }
}

fn redacted_arguments(action: &QaDriverActionV1) -> std::collections::BTreeMap<String, String> {
    let mut result = std::collections::BTreeMap::new();
    match action {
        QaDriverActionV1::TypeText { text } => {
            result.insert(
                "text".into(),
                format!("[redacted; {} chars]", text.chars().count()),
            );
        }
        QaDriverActionV1::KeyChord { keys } => {
            result.insert("keys".into(), keys.join("+"));
        }
        QaDriverActionV1::Click { window_id, x, y }
        | QaDriverActionV1::DoubleClick { window_id, x, y } => {
            result.insert("window_id".into(), window_id.clone());
            result.insert("point".into(), format!("{x},{y}"));
        }
        QaDriverActionV1::FocusWindow { window_id }
        | QaDriverActionV1::CloseWindow { window_id } => {
            result.insert("window_id".into(), window_id.clone());
        }
        QaDriverActionV1::AssertRegionVisible {
            window_id,
            x,
            y,
            width,
            height,
        } => {
            result.insert("window_id".into(), window_id.clone());
            result.insert("region".into(), format!("{x},{y},{width},{height}"));
        }
        _ => {}
    }
    result
}
