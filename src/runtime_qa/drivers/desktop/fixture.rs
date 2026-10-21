mod chromium;
pub(super) mod egui;
mod encoding;
mod support;
pub(super) use support::{
    FixtureProcess, screenshot, sha256, step, visible_region_state, wait_for_click_observation,
    wait_for_title,
};
pub(super) mod window;

pub(super) fn run_egui_window() -> Result<(), String> {
    egui::run_window()
}

use std::time::Duration;

use crate::runtime_qa::QaDriverActionV1;

use super::{actions, display::DesktopDisplay};

pub(super) fn exercise() -> Result<serde_json::Value, String> {
    let interaction = run_case(false)?;
    let seeded_defect = run_case(true)?;
    let visible_control = visibility_case(false)?;
    let absent_control = visibility_case(true)?;
    let packaged_chromium = chromium::exercise()?;
    let egui_application = egui::exercise()?;
    if interaction["click_assertion"] != "passed"
        || interaction["keyboard_assertion"] != "passed"
        || interaction["screenshot_changed"] != true
        || interaction["screenshot_hashes_distinct"] != true
        || seeded_defect["click_assertion"] != "failed"
        || seeded_defect["defect_detected"] != true
        || !visible_region_state(&visible_control, true)
        || !visible_region_state(&absent_control, false)
    {
        return Err("desktop fixture did not prove interactions and defect detection".into());
    }
    Ok(serde_json::json!({
        "schema_version": 1,
        "status": "passed",
        "driver_version": "linux-x11-driver/v1",
        "fixture": "synthetic-x11-counter-v1",
        "interaction_case": interaction,
        "seeded_defect_case": seeded_defect,
        "native_control_visibility": {"visible": visible_control, "absent": absent_control},
        "packaged_non_rust_application": packaged_chromium,
        "egui_application": egui_application,
    }))
}

fn run_case(ignore_click: bool) -> Result<serde_json::Value, String> {
    let display = DesktopDisplay::start()?;
    let mut fixture = FixtureProcess::start(ignore_click, "Target", false)?;
    let mut foreign = FixtureProcess::start(false, "Foreign", false)?;
    let owner_pids = super::process::descendants(fixture.0.id());
    let window = wait_for_title(&display, "Ready", Duration::from_secs(5), &owner_pids)?;
    let foreign_pids = super::process::descendants(foreign.0.id());
    let foreign_window = wait_for_title(&display, "Ready", Duration::from_secs(5), &foreign_pids)?;
    if display
        .windows(&owner_pids)?
        .iter()
        .any(|candidate| candidate.id == foreign_window.id)
    {
        return Err("foreign process window was included in the approved target window set".into());
    }
    let foreign_action = step(QaDriverActionV1::FocusWindow {
        window_id: foreign_window.id.to_string(),
    });
    if actions::execute(
        &display,
        &foreign_action,
        Duration::from_secs(1),
        &owner_pids,
    )
    .is_ok()
    {
        return Err("desktop driver accepted a foreign process window".into());
    }
    // Both synthetic windows open at the same coordinates. Remove the foreign
    // window after the ownership assertion so it cannot cover the target while
    // the XTEST click is routed by pointer position.
    foreign.stop();
    let window_id = window.id.to_string();
    let before = display.screenshot_png()?;
    let focus = step(QaDriverActionV1::FocusWindow {
        window_id: window_id.clone(),
    });
    actions::execute(&display, &focus, Duration::from_secs(2), &owner_pids)?;
    let click = step(QaDriverActionV1::Click {
        window_id: window_id.clone(),
        x: 55,
        y: 58,
    });
    actions::execute(&display, &click, Duration::from_secs(2), &owner_pids)?;
    let after_click_window = wait_for_click_observation(&display, ignore_click, &owner_pids)?;
    let observed = actions::window::describe(std::slice::from_ref(&after_click_window));
    let click_passed = super::run::expected_state_met("Clicked", &observed);
    let after_click = display.screenshot_png()?;
    let mut action_evidence = vec![serde_json::json!({
        "action": "mouse_click",
        "target_window": window_id,
        "point": {"x": 55, "y": 58},
        "expected_state": "Clicked",
        "observed_state": observed,
        "status": if click_passed { "passed" } else { "failed" },
    })];

    let mut keyboard_passed = false;
    let mut after_keyboard = after_click.clone();
    if !ignore_click {
        let keyboard = step(QaDriverActionV1::TypeText { text: "ok".into() });
        actions::execute(&display, &keyboard, Duration::from_secs(2), &owner_pids)?;
        let typed_window =
            wait_for_title(&display, "Typed:ok", Duration::from_secs(2), &owner_pids)?;
        let typed_state = actions::window::describe(&[typed_window]);
        keyboard_passed = super::run::expected_state_met("Typed:ok", &typed_state);
        action_evidence.push(serde_json::json!({
            "action": "keyboard_type",
            "text_redacted": "[2 characters]",
            "expected_state": "Typed:ok",
            "observed_state": typed_state,
            "status": if keyboard_passed { "passed" } else { "failed" },
        }));
        after_keyboard = display.screenshot_png()?;
    }
    fixture.stop();
    let before_sha = sha256(&before);
    let click_sha = sha256(&after_click);
    let keyboard_sha = sha256(&after_keyboard);
    Ok(serde_json::json!({
        "click_assertion": if click_passed { "passed" } else { "failed" },
        "keyboard_assertion": if keyboard_passed { "passed" } else if ignore_click { "not_run" } else { "failed" },
        "defect_detected": ignore_click && !click_passed,
        "foreign_window_excluded": true,
        "foreign_window_action_rejected": true,
        "actions": action_evidence,
        "screenshot_changed": before_sha != click_sha,
        "screenshots": [
            screenshot("before_click", &before),
            screenshot("after_click", &after_click),
            screenshot("after_keyboard", &after_keyboard),
        ],
        "screenshot_hashes_distinct": before_sha != click_sha && click_sha != keyboard_sha,
    }))
}

fn visibility_case(hide_control: bool) -> Result<String, String> {
    let display = DesktopDisplay::start()?;
    let mut fixture = FixtureProcess::start(false, "Visibility", hide_control)?;
    let owner_pids = super::process::descendants(fixture.0.id());
    let window = wait_for_title(&display, "Ready", Duration::from_secs(5), &owner_pids)?;
    let action = step(QaDriverActionV1::AssertRegionVisible {
        window_id: window.id.to_string(),
        x: 20,
        y: 36,
        width: 200,
        height: 52,
    });
    let result = actions::execute(&display, &action, Duration::from_secs(1), &owner_pids)?;
    fixture.stop();
    match result {
        actions::ActionResult::State(state) => Ok(state),
        _ => Err("native control visibility probe did not return an observation".into()),
    }
}
