mod encoding;
pub(super) mod window;

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
    if interaction["click_assertion"] != "passed"
        || interaction["keyboard_assertion"] != "passed"
        || interaction["screenshot_changed"] != true
        || interaction["screenshot_hashes_distinct"] != true
        || seeded_defect["click_assertion"] != "failed"
        || seeded_defect["defect_detected"] != true
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
    }))
}

fn run_case(ignore_click: bool) -> Result<serde_json::Value, String> {
    let display = DesktopDisplay::start()?;
    let mut fixture = FixtureProcess::start(ignore_click)?;
    let window = wait_for_title(&display, "Ready", Duration::from_secs(5))?;
    let window_id = window.id.to_string();
    let before = display.screenshot_png()?;
    let focus = step(QaDriverActionV1::FocusWindow {
        window_id: window_id.clone(),
    });
    actions::execute(&display, &focus, Duration::from_secs(2))?;
    let click = step(QaDriverActionV1::Click {
        window_id: window_id.clone(),
        x: 55,
        y: 58,
    });
    actions::execute(&display, &click, Duration::from_secs(2))?;
    let after_click_window = wait_for_click_observation(&display, ignore_click)?;
    let observed = actions::describe_windows(std::slice::from_ref(&after_click_window));
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
        actions::execute(&display, &keyboard, Duration::from_secs(2))?;
        let typed_window = wait_for_title(&display, "Typed:ok", Duration::from_secs(2))?;
        let typed_state = actions::describe_windows(&[typed_window]);
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
) -> Result<super::display::WindowInfo, String> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(window) = display
            .windows()?
            .into_iter()
            .find(|window| window.title.contains(expected))
        {
            return Ok(window);
        }
        if Instant::now() >= deadline {
            return Err(format!("desktop fixture did not report state {expected}"));
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn wait_for_click_observation(
    display: &DesktopDisplay,
    ignore_click: bool,
) -> Result<super::display::WindowInfo, String> {
    // The fixture runs under the same CPU and PID limits as a real QA session;
    // X11 event delivery can lag on a loaded hosted runner after the synthetic
    // click has been sent. Keep this bounded, but give the event loop time to
    // observe the resulting title change.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let window = display
            .windows()?
            .into_iter()
            .find(|window| window.title.starts_with("Ready") || window.title.starts_with("Clicked"))
            .ok_or_else(|| "desktop fixture window disappeared after mouse input".to_owned())?;
        let clicked = window.title.starts_with("Clicked");
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
    fn start(ignore_click: bool) -> Result<Self, String> {
        let executable = std::env::current_exe()
            .map_err(|error| format!("could not locate desktop QA driver: {error}"))?;
        Command::new(executable)
            .arg("--fixture-window")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", "/tmp")
            .env("DISPLAY", ":99")
            .env("LANG", "C.UTF-8")
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
