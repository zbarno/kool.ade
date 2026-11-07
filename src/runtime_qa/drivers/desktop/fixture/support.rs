use std::{
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use sha2::Digest;

use crate::runtime_qa::{QaDriverActionV1, QaScenarioStepV1};

use super::super::display::DesktopDisplay;

pub fn step(driver_action: QaDriverActionV1) -> QaScenarioStepV1 {
    QaScenarioStepV1 {
        driver_action,
        expected_state: None,
        screenshot_checkpoint: false,
    }
}

pub fn visible_region_state(state: &str, visible: bool) -> bool {
    let Some((window, observation)) = state.split_once(": region ") else {
        return false;
    };
    window
        .strip_prefix("window ")
        .is_some_and(|id| id.parse::<u32>().is_ok())
        && observation == if visible { "visible" } else { "not visible" }
}

pub fn wait_for_title(
    display: &DesktopDisplay,
    expected: &str,
    timeout: Duration,
    owner_pids: &std::collections::BTreeSet<u32>,
) -> Result<super::super::display::WindowInfo, String> {
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

pub fn wait_for_click_observation(
    display: &DesktopDisplay,
    ignore_click: bool,
    owner_pids: &std::collections::BTreeSet<u32>,
) -> Result<super::super::display::WindowInfo, String> {
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

/// Wait for the synthetic fixture's state-independent title text to
/// actually render before taking the baseline screenshot or testing control
/// visibility. WM_NAME may be updated before an X11 Expose event is painted.
pub fn wait_for_fixture_paint(
    display: &DesktopDisplay,
    window: u32,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        // The fixture draws its fixed heading at x=24,y=32, irrespective of
        // the click, keyboard or intentionally hidden control state.
        if display.region_has_visible_content(window, 20, 0, 220, 36)? {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("desktop fixture never painted its baseline heading".into());
        }
        thread::sleep(Duration::from_millis(50));
    }
}

/// A window title can update before the X server has processed the drawing
/// requests. Require the screenshot to reflect a real pixel change rather
/// than weakening the click and keyboard assertions on busy CI runners.
pub fn wait_for_screenshot_change(
    display: &DesktopDisplay,
    previous: &[u8],
    stage: &str,
) -> Result<Vec<u8>, String> {
    let previous_hash = sha256(previous);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let current = display.screenshot_png()?;
        if sha256(&current) != previous_hash {
            return Ok(current);
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "desktop fixture {stage} changed its window title but not the rendered pixels"
            ));
        }
        thread::sleep(Duration::from_millis(50));
    }
}

pub fn screenshot(name: &str, bytes: &[u8]) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "mime": "image/png",
        "sha256": sha256(bytes),
        "width": 1440,
        "height": 900,
        "bytes_base64": super::encoding::base64(bytes),
    })
}

pub fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", sha2::Sha256::digest(bytes))
}

pub struct FixtureProcess(pub(super) Child);

impl FixtureProcess {
    pub fn start(ignore_click: bool, title: &str, hide_control: bool) -> Result<Self, String> {
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

    pub fn stop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Drop for FixtureProcess {
    fn drop(&mut self) {
        self.stop();
    }
}
