use std::time::{SystemTime, UNIX_EPOCH};

use crate::runtime_qa::QaDriverActionV1;

pub(super) fn supports_generic_computer_action(action: &QaDriverActionV1) -> bool {
    matches!(
        action,
        QaDriverActionV1::WaitForWindow
            | QaDriverActionV1::ListWindows
            | QaDriverActionV1::FocusWindow { .. }
            | QaDriverActionV1::ResizeWindow { .. }
            | QaDriverActionV1::Click { .. }
            | QaDriverActionV1::DoubleClick { .. }
            | QaDriverActionV1::Drag { .. }
            | QaDriverActionV1::TypeText { .. }
            | QaDriverActionV1::KeyChord { .. }
            | QaDriverActionV1::Scroll { .. }
            | QaDriverActionV1::AssertRegionVisible { .. }
    )
}

pub(super) fn action_name(action: &QaDriverActionV1) -> Result<String, String> {
    serde_json::to_value(action)
        .ok()
        .and_then(|value| {
            value
                .get("action")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .ok_or_else(|| "Computer QA action could not be identified".into())
}

pub(super) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}
