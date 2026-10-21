mod devtools;
mod local_http;
mod process;
mod render;

use std::{fs, time::Duration};

use crate::runtime_qa::{QaDriverActionV1, QaScenarioStepV1};

use super::super::{actions, display::DesktopDisplay};

// Boundary: launch the pinned image's packaged Chromium GUI under private Xvfb
// and drive it through the production X11 action path. Unit tests cannot prove
// a non-Rust process creates owned windows or responds to real XTEST input.
// External dependency: rootless Docker and the pinned Playwright image.
// Setup cost: one image build and a short headed Chromium smoke.
pub(super) fn exercise() -> Result<serde_json::Value, String> {
    let (executable, version) = process::chromium_executable()?;
    let executable_sha256 = process::sha256(&fs::read(&executable).map_err(process::io_error)?);
    let root = std::env::temp_dir().join(format!(
        "koolade-runtime-qa-chromium-fixture-{}",
        std::process::id()
    ));
    fs::create_dir(&root).map_err(process::io_error)?;
    let _cleanup = process::FixtureDirectory(root.clone());
    let page_server = local_http::LocalFixture::start(FIXTURE_HTML)?;
    let display = DesktopDisplay::start()?;
    let profile = root.join("profile");
    let mut app = process::ChromiumProcess::start(&executable, &root)?;
    if let Err(error) = devtools::open_page(&profile, &page_server.url()) {
        let exit = app
            .0
            .try_wait()
            .ok()
            .flatten()
            .map(|status| status.to_string());
        let diagnostic = fs::read_to_string(root.join("chromium.stderr"))
            .unwrap_or_default()
            .chars()
            .take(2048)
            .collect::<String>();
        return Err(format!(
            "{error}; Chromium exit: {exit:?}; stderr: {diagnostic:?}"
        ));
    }
    let app_pid = app.0.id();
    let window = match render::wait_for_window(&display, Duration::from_secs(10), app_pid) {
        Ok(window) => window,
        Err(error) => {
            let exit = app
                .0
                .try_wait()
                .ok()
                .flatten()
                .map(|status| status.to_string());
            let diagnostic = fs::read_to_string(root.join("chromium.stderr"))
                .unwrap_or_default()
                .chars()
                .take(2048)
                .collect::<String>();
            return Err(format!(
                "{error}; Chromium exit: {exit:?}; stderr: {diagnostic:?}"
            ));
        }
    };
    let owner_pids = super::super::process::descendants(app_pid);
    let window_id = window.id.to_string();
    actions::execute(
        &display,
        &step(QaDriverActionV1::FocusWindow {
            window_id: window_id.clone(),
        }),
        Duration::from_secs(2),
        &owner_pids,
    )?;
    let state_pixel = render::wait_for_color(&display, Duration::from_secs(5), (208, 32, 32))?;
    let click = (
        render::relative_coordinate(state_pixel.0, window.x, 300, 55)?,
        render::relative_coordinate(state_pixel.1, window.y, 36, 58)?,
    );
    let input = (
        render::relative_coordinate(state_pixel.0, window.x, 300, 80)?,
        render::relative_coordinate(state_pixel.1, window.y, 36, 140)?,
    );
    let before = display.screenshot_png()?;
    actions::execute(
        &display,
        &step(QaDriverActionV1::Click {
            window_id: window_id.clone(),
            x: click.0,
            y: click.1,
        }),
        Duration::from_secs(2),
        &owner_pids,
    )?;
    render::wait_for_pixel(&display, Duration::from_secs(5), state_pixel, (32, 208, 96))?;
    let after_click = display.screenshot_png()?;
    actions::execute(
        &display,
        &step(QaDriverActionV1::Click {
            window_id: window_id.clone(),
            x: input.0,
            y: input.1,
        }),
        Duration::from_secs(2),
        &owner_pids,
    )?;
    actions::execute(
        &display,
        &step(QaDriverActionV1::TypeText { text: "ok".into() }),
        Duration::from_secs(2),
        &owner_pids,
    )?;
    render::wait_for_pixel(&display, Duration::from_secs(5), state_pixel, (32, 96, 208))?;
    let after_type = display.screenshot_png()?;
    actions::execute(
        &display,
        &step(QaDriverActionV1::ResizeWindow {
            window_id: window_id.clone(),
            width: 900,
            height: 600,
        }),
        Duration::from_secs(2),
        &owner_pids,
    )?;
    let resized = display.window(window.id, &owner_pids)?;
    if resized.width != 900 || resized.height != 600 {
        return Err("packaged Chromium fixture did not resize".into());
    }
    app.stop();
    if process::sha256(&before) == process::sha256(&after_click)
        || process::sha256(&after_click) == process::sha256(&after_type)
    {
        return Err("packaged Chromium fixture screenshots did not reflect interaction".into());
    }
    Ok(serde_json::json!({
        "status": "passed",
        "application": "packaged-chromium-local-page",
        "chromium_version": version,
        "executable_sha256": executable_sha256,
        "click_assertion": "passed",
        "keyboard_assertion": "passed",
        "resize": {"width": resized.width, "height": resized.height},
        "screenshot_hashes_distinct": true,
        "screenshots": [
            render::screenshot("before_click", &before),
            render::screenshot("after_click", &after_click),
            render::screenshot("after_keyboard", &after_type),
        ]
    }))
}

fn step(driver_action: QaDriverActionV1) -> QaScenarioStepV1 {
    QaScenarioStepV1 {
        driver_action,
        expected_state: None,
        screenshot_checkpoint: false,
    }
}

const FIXTURE_HTML: &str = r##"<!doctype html>
<meta charset="utf-8">
<title>Ready</title>
<style>body{font:20px sans-serif;background:#edf2f7}button,input{position:absolute;left:20px;width:200px;height:52px}button{top:36px}input{top:120px}#state{position:absolute;left:300px;top:36px;width:100px;height:100px;background:#d02020}</style>
<button onclick="document.querySelector('#state').style.background='#20d060'">Click fixture</button>
<input aria-label="Fixture input" oninput="document.querySelector('#state').style.background='#2060d0'">
<div id="state"></div>
"##;
