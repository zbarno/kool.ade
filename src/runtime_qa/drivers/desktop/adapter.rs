use crate::runtime_qa::{QaRunRequestV1, QaScenarioV1, QaValidatedTargetV1};

use super::{
    display::{DISPLAY_HEIGHT, DISPLAY_WIDTH, DesktopDisplay},
    run,
};

const MAX_DRIVER_REQUEST_BYTES: usize = 256 * 1024;
const MAX_DRIVER_RESPONSE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DesktopDriverRequestV1 {
    schema_version: u16,
    request: QaRunRequestV1,
    scenario: QaScenarioV1,
    target: QaValidatedTargetV1,
}

/// Execute one bounded request inside the isolated desktop QA image.
pub fn execute_request_json(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.is_empty() || bytes.len() > MAX_DRIVER_REQUEST_BYTES {
        return Err("desktop QA request exceeds its byte limit".into());
    }
    let envelope: DesktopDriverRequestV1 =
        serde_json::from_slice(bytes).map_err(|_| "desktop QA request is malformed")?;
    if envelope.schema_version != 1 {
        return Err("desktop QA request schema is unsupported".into());
    }
    let output = run::run(&envelope.request, &envelope.scenario, &envelope.target)?;
    let response =
        serde_json::to_vec(&output).map_err(|_| "desktop QA response could not encode")?;
    if response.len() as u64
        > MAX_DRIVER_RESPONSE_BYTES.min(
            envelope
                .request
                .bounded_limits
                .max_evidence_bytes
                .saturating_add(1024 * 1024),
        )
    {
        return Err("desktop QA response exceeds its evidence limit".into());
    }
    Ok(response)
}

/// Minimal image smoke used by CI to verify Xvfb and screenshot capture.
pub fn smoke_display() -> Result<(u16, u16, usize), String> {
    let display = DesktopDisplay::start()?;
    let screenshot = display.screenshot_png()?;
    if screenshot.is_empty() {
        return Err("private Xvfb screenshot was empty".into());
    }
    Ok((DISPLAY_WIDTH, DISPLAY_HEIGHT, screenshot.len()))
}

/// Exercise actual X11 mouse and keyboard actions against a synthetic window.
pub fn smoke_fixture() -> Result<serde_json::Value, String> {
    super::fixture::exercise()
}

/// Child-process entrypoint for the fixture window used by `smoke_fixture`.
pub fn run_fixture_window() -> Result<(), String> {
    super::fixture::window::run()
}

pub fn run_egui_fixture_window() -> Result<(), String> {
    super::fixture::run_egui_window()
}
