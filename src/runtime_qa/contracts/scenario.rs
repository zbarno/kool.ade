use serde::{Deserialize, Serialize};

mod action_platform;
mod proposal;
mod validation;

use super::{is_uuid, valid_text};
use action_platform::action_platform;
pub use proposal::QaScenarioProposalV1;
pub use validation::validate_scenario_set;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum QaDriverActionV1 {
    LaunchValidatedTarget,
    WaitForWindow,
    ListWindows,
    FocusWindow {
        window_id: String,
    },
    ResizeWindow {
        window_id: String,
        width: u16,
        height: u16,
    },
    Click {
        window_id: String,
        x: u16,
        y: u16,
    },
    DoubleClick {
        window_id: String,
        x: u16,
        y: u16,
    },
    Drag {
        window_id: String,
        start_x: u16,
        start_y: u16,
        end_x: u16,
        end_y: u16,
    },
    TypeText {
        text: String,
    },
    KeyChord {
        keys: Vec<String>,
    },
    Scroll {
        window_id: String,
        delta_x: i16,
        delta_y: i16,
    },
    WaitForVisible {
        selector: String,
    },
    /// Checks that a bounded native window region contains visible pixel variation.
    /// This reports visual presence and does not claim semantic accessibility labels.
    AssertRegionVisible {
        window_id: String,
        x: u16,
        y: u16,
        width: u16,
        height: u16,
    },
    NavigateAllowedUrl {
        url: String,
    },
    FindByRole {
        role: String,
        name: String,
    },
    FindByTestId {
        test_id: String,
    },
    Fill {
        selector: String,
        value: String,
    },
    Press {
        selector: String,
        key: String,
    },
    Select {
        selector: String,
        value: String,
    },
    Screenshot,
    ReadAppLogs,
    CollectConsole,
    CollectNetworkFailures,
    BrowserOpen {
        url: String,
    },
    BrowserClick {
        locator: QaBrowserLocatorV1,
    },
    BrowserFill {
        locator: QaBrowserLocatorV1,
        value: String,
    },
    BrowserPress {
        locator: QaBrowserLocatorV1,
        key: String,
    },
    BrowserSelect {
        locator: QaBrowserLocatorV1,
        value: String,
    },
    BrowserWaitForVisible {
        locator: QaBrowserLocatorV1,
    },
    BrowserAssertText {
        locator: QaBrowserLocatorV1,
        expected_text: String,
    },
    BrowserAssertUrl {
        url: String,
    },
    BrowserSetViewport {
        width: u16,
        height: u16,
    },
    BrowserScreenshot,
    BrowserScroll {
        locator: QaBrowserLocatorV1,
    },
    CloseWindow {
        window_id: String,
    },
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "locator", rename_all = "snake_case", deny_unknown_fields)]
pub enum QaBrowserLocatorV1 {
    Role { role: String, name: String },
    TestId { test_id: String },
    Label { label: String },
}

impl QaBrowserLocatorV1 {
    fn validate(&self) -> bool {
        match self {
            Self::Role { role, name } => valid_text(role, 64) && valid_text(name, 512),
            Self::TestId { test_id } => valid_text(test_id, 512),
            Self::Label { label } => valid_text(label, 512),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QaScenarioStepV1 {
    pub driver_action: QaDriverActionV1,
    pub expected_state: Option<String>,
    pub screenshot_checkpoint: bool,
}

/// Operator-approved screenshot region that is blacked out before evidence is
/// hashed or persisted. Coordinates are in screenshot pixels.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QaRedactionRegionV1 {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub redaction_regions: Vec<QaRedactionRegionV1>,
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
            || self.redaction_regions.len() > 64
            || self.redaction_regions.iter().any(|region| {
                region.width == 0
                    || region.height == 0
                    || u32::from(region.x) + u32::from(region.width) > 7680
                    || u32::from(region.y) + u32::from(region.height) > 4320
            })
            || self.priority > 5
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
            QaDriverActionV1::BrowserSetViewport { width, height } => {
                (320..=3840).contains(width) && (240..=2160).contains(height)
            }
            QaDriverActionV1::NavigateAllowedUrl { url } => {
                valid_text(url, 2048) && url.starts_with("http://qa-service/")
            }
            _ => true,
        }
}
