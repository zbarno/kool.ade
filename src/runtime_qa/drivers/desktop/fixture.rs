mod chromium;
pub(super) mod egui;
mod encoding;
pub(super) mod window;

pub(super) fn run_egui_window() -> Result<(), String> {
    egui::run_window()
}

use std::{
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use sha2::Digest;

use crate::runtime_qa::{QaDriverActionV1, QaScenarioStepV1};

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
        || visible_control != "region visible"
        || absent_control != "region not visible"
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

fn step(driver_action: QaDriverActionV1) -> QaScenarioStepV1 {
    QaScenarioStepV1 {
        driver_action,
        expected_state: None,
        screenshot_checkpoint: false,
    }
}

fn wait_for_title(
    display: &DesktopDisplay,
    expected: &str,
    timeout: Duration,
    owner_pids: &std::collections::BTreeSet<u32>,
) -> Result<super::display::WindowInfo, String> {
    let deadline = Instant::now() + timeout;
    loop {
        let windows = display.windows(owner_pids)?;
        if let Some(window) = windows
            .iter()
            .find(|window| window.title.contains(expected))
            .cloned()
        {
            return Ok(window);
        }
        if Instant::now() >= deadline {
            let observed = windows
                .iter()
                .map(|window| {
                    format!(
                        "{:?} at {},{} size {}x{}",
                        window.title, window.x, window.y, window.width, window.height
                    )
                })
                .collect::<Vec<_>>();
            return Err(format!(
                "desktop fixture did not report state {expected}; observed windows: {observed:?}"
            ));
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn wait_for_click_observation(
    display: &DesktopDisplay,
    ignore_click: bool,
    owner_pids: &std::collections::BTreeSet<u32>,
) -> Result<super::display::WindowInfo, String> {
    // The fixture runs under the same CPU and PID limits as a real QA session;
    // X11 event delivery can lag on a loaded hosted runner after the synthetic
    // click has been sent. Keep this bounded, but give the event loop time to
    // observe the resulting title change.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let window = display
            .windows(owner_pids)?
            .into_iter()
            .find(|window| window.title.contains(":Ready") || window.title.contains(":Clicked"))
            .ok_or_else(|| "desktop fixture window disappeared after mouse input".to_owned())?;
        let clicked = window.title.contains(":Clicked");
        if clicked || Instant::now() >= deadline {
            if ignore_click && clicked {
                return Ok(window);
            }
            if !ignore_click && clicked {
                return Ok(window);
            }
            if ignore_click {
                return Ok(window);
            }
            return Err(format!(
                "desktop fixture did not respond to the actual mouse click; observed title {:?} at {},{} size {}x{}",
                window.title, window.x, window.y, window.width, window.height
            ));
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn screenshot(name: &str, bytes: &[u8]) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "mime": "image/png",
        "sha256": sha256(bytes),
        "width": 1440,
        "height": 900,
        "bytes_base64": encoding::base64(bytes),
    })
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", sha2::Sha256::digest(bytes))
}

struct FixtureProcess(Child);

impl FixtureProcess {
    fn start(ignore_click: bool, title: &str, hide_control: bool) -> Result<Self, String> {
        let executable = std::env::current_exe()
            .map_err(|error| format!("could not locate desktop QA driver: {error}"))?;
        Command::new(executable)
            .arg("--fixture-window")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", "/tmp")
            .env("DISPLAY", ":99")
            .env("LANG", "C.UTF-8")
            .env("QA_FIXTURE_TITLE", title)
            .env(
                "QA_FIXTURE_HIDE_CONTROL",
                if hide_control { "1" } else { "0" },
            )
            .env(
                "QA_FIXTURE_IGNORE_CLICK",
                if ignore_click { "1" } else { "0" },
            )
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(Self)
            .map_err(|error| format!("could not launch desktop fixture: {error}"))
    }

    fn stop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Drop for FixtureProcess {
    fn drop(&mut self) {
        self.stop();
    }
}
