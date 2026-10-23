use serde::{Deserialize, Serialize};

use super::super::valid_text;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum QaHttpMethodV1 {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

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
    /// Makes one bounded JSON API request through the leased task-service relay.
    /// Request/response bodies are never copied into QA logs or evidence.
    ApiRequest {
        method: QaHttpMethodV1,
        path: String,
        json_body: Option<String>,
        expected_status: u16,
        expected_body_contains: Option<String>,
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
    pub(super) fn validate(&self) -> bool {
        match self {
            Self::Role { role, name } => valid_text(role, 64) && valid_text(name, 512),
            Self::TestId { test_id } => valid_text(test_id, 512),
            Self::Label { label } => valid_text(label, 512),
        }
    }
}
