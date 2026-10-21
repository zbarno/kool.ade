use serde::{Deserialize, Serialize};

mod action;
mod action_platform;

pub use action::{QaBrowserLocatorV1, QaDriverActionV1, QaHttpMethodV1};

use super::{is_uuid, valid_text};
use action_platform::action_platform;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QaScenarioStepV1 {
    pub driver_action: QaDriverActionV1,
    pub expected_state: Option<String>,
    pub screenshot_checkpoint: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QaScenarioV1 {
    pub schema_version: u16,
    pub id: String,
    pub requirement_ids: Vec<String>,
    pub preconditions: Vec<String>,
    pub steps: Vec<QaScenarioStepV1>,
    pub expected_results: Vec<String>,
    pub negative_paths: Vec<String>,
    pub visual_expectations: Vec<String>,
    pub priority: u8,
}

impl QaScenarioV1 {
    pub fn validate(&self, request: &super::QaRunRequestV1) -> Result<(), String> {
        let limits = &request.bounded_limits;
        if self.schema_version != 1
            || !is_uuid(&self.id)
            || self.requirement_ids.is_empty()
            || self.requirement_ids.len() > 256
            || self
                .requirement_ids
                .iter()
                .any(|id| !request.requirement_ids.contains(id))
            || self.preconditions.len() > 128
            || self.steps.is_empty()
            || self.steps.len() > limits.max_actions_per_scenario as usize
            || self.expected_results.is_empty()
            || self.expected_results.len() > 128
            || self.negative_paths.len() > 128
            || self.visual_expectations.len() > 128
            || self.priority > 5
            || (request.relevant_platforms.as_slice() == [super::QaPlatform::HeadlessService]
                && (!self.visual_expectations.is_empty()
                    || self.steps.iter().any(|step| step.screenshot_checkpoint)))
            || self
                .preconditions
                .iter()
                .chain(self.expected_results.iter())
                .chain(self.negative_paths.iter())
                .chain(self.visual_expectations.iter())
                .any(|value| !valid_text(value, 4096))
            || self.steps.iter().any(|step| {
                !valid_step(step)
                    || action_platform(&step.driver_action)
                        .is_some_and(|platform| !request.relevant_platforms.contains(&platform))
            })
        {
            return Err("runtime QA scenario is invalid or exceeds its request bounds".into());
        }
        Ok(())
    }
}

fn valid_step(step: &QaScenarioStepV1) -> bool {
    if matches!(
        &step.driver_action,
        QaDriverActionV1::AssertRegionVisible { .. }
    ) && step.expected_state.as_deref() != Some("region visible")
    {
        return false;
    }
    step.expected_state
        .as_ref()
        .is_none_or(|state| valid_text(state, 4096))
        && match &step.driver_action {
            QaDriverActionV1::FocusWindow { window_id }
            | QaDriverActionV1::ResizeWindow { window_id, .. }
            | QaDriverActionV1::Click { window_id, .. }
            | QaDriverActionV1::DoubleClick { window_id, .. }
            | QaDriverActionV1::Drag { window_id, .. }
            | QaDriverActionV1::Scroll { window_id, .. }
            | QaDriverActionV1::CloseWindow { window_id } => valid_text(window_id, 128),
            QaDriverActionV1::AssertRegionVisible {
                window_id,
                width,
                height,
                ..
            } => {
                valid_text(window_id, 128)
                    && (4..=256).contains(width)
                    && (4..=256).contains(height)
            }
            QaDriverActionV1::TypeText { text } => text.len() <= 16 * 1024 && !text.contains('\0'),
            QaDriverActionV1::KeyChord { keys } => {
                !keys.is_empty() && keys.len() <= 8 && keys.iter().all(|key| valid_text(key, 32))
            }
            QaDriverActionV1::WaitForVisible { selector }
            | QaDriverActionV1::FindByTestId { test_id: selector } => valid_text(selector, 512),
            QaDriverActionV1::Fill { selector, value }
            | QaDriverActionV1::Select { selector, value } => {
                valid_text(selector, 512) && value.len() <= 4096 && !value.contains('\0')
            }
            QaDriverActionV1::Press { selector, key } => {
                valid_text(selector, 512) && valid_text(key, 32)
            }
            QaDriverActionV1::FindByRole { role, name } => {
                valid_text(role, 64) && valid_text(name, 512)
            }
            QaDriverActionV1::BrowserOpen { url } | QaDriverActionV1::BrowserAssertUrl { url } => {
                valid_text(url, 2048) && url.starts_with("http://qa-service/")
            }
            QaDriverActionV1::BrowserClick { locator }
            | QaDriverActionV1::BrowserWaitForVisible { locator }
            | QaDriverActionV1::BrowserScroll { locator } => locator.validate(),
            QaDriverActionV1::BrowserFill { locator, value }
            | QaDriverActionV1::BrowserSelect { locator, value } => {
                locator.validate() && value.len() <= 4096 && !value.contains('\0')
            }
            QaDriverActionV1::BrowserPress { locator, key } => {
                locator.validate() && valid_text(key, 32)
            }
            QaDriverActionV1::BrowserAssertText {
                locator,
                expected_text,
            } => locator.validate() && valid_text(expected_text, 4096),
            QaDriverActionV1::ApiRequest {
                method,
                path,
                json_body,
                expected_status,
                expected_body_contains,
                ..
            } => {
                valid_service_path(path)
                    && !(*method == QaHttpMethodV1::Get && json_body.is_some())
                    && (100..=599).contains(expected_status)
                    && json_body.as_ref().is_none_or(|body| {
                        body.len() <= 16 * 1024
                            && !body.contains('\0')
                            && serde_json::from_str::<serde_json::Value>(body).is_ok()
                    })
                    && expected_body_contains.as_ref().is_none_or(|text| {
                        !text.is_empty() && text.len() <= 4096 && !text.contains('\0')
                    })
                    && step.expected_state.is_some()
                    && !step.screenshot_checkpoint
            }
            QaDriverActionV1::BrowserSetViewport { width, height } => {
                (320..=3840).contains(width) && (240..=2160).contains(height)
            }
            QaDriverActionV1::NavigateAllowedUrl { url } => {
                valid_text(url, 2048) && url.starts_with("http://qa-service/")
            }
            _ => true,
        }
}

fn valid_service_path(path: &str) -> bool {
    path.starts_with('/')
        && !path.starts_with("//")
        && path.len() <= 2048
        && !path.contains(['?', '#', '\\', '\0'])
        && !path.chars().any(char::is_control)
        && !path.split('/').any(|part| matches!(part, "." | ".."))
}

pub fn validate_scenario_set(
    request: &super::QaRunRequestV1,
    scenarios: &[QaScenarioV1],
) -> Result<(), String> {
    request.validate()?;
    if scenarios.is_empty() || scenarios.len() > request.bounded_limits.max_scenarios as usize {
        return Err("runtime QA scenario count exceeds the request bounds".into());
    }
    let mut ids = std::collections::BTreeSet::new();
    for scenario in scenarios {
        scenario.validate(request)?;
        if !ids.insert(scenario.id.as_str()) {
            return Err("runtime QA scenario IDs must be unique".into());
        }
    }
    let covered = scenarios
        .iter()
        .flat_map(|scenario| scenario.requirement_ids.iter())
        .collect::<std::collections::BTreeSet<_>>();
    if request
        .requirement_ids
        .iter()
        .any(|id| !covered.contains(id))
    {
        return Err("runtime QA scenarios do not cover every approved requirement".into());
    }
    let total_actions: usize = scenarios.iter().map(|scenario| scenario.steps.len()).sum();
    if total_actions > request.bounded_limits.max_total_actions as usize {
        return Err("runtime QA total action count exceeds the request bounds".into());
    }
    Ok(())
}
