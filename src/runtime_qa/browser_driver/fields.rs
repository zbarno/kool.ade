use crate::runtime_qa::QaDriverActionV1;
use serde_json::{Map, Value};
use url::Url;

pub(super) fn action_name(action: &QaDriverActionV1) -> &'static str {
    match action {
        QaDriverActionV1::BrowserOpen { .. } => "browser_open",
        QaDriverActionV1::BrowserClick { .. } => "browser_click",
        QaDriverActionV1::BrowserFill { .. } => "browser_fill",
        QaDriverActionV1::BrowserPress { .. } => "browser_press",
        QaDriverActionV1::BrowserSelect { .. } => "browser_select",
        QaDriverActionV1::BrowserWaitForVisible { .. } => "browser_wait_for_visible",
        QaDriverActionV1::BrowserAssertText { .. } => "browser_assert_text",
        QaDriverActionV1::BrowserAssertUrl { .. } => "browser_assert_url",
        QaDriverActionV1::BrowserSetViewport { .. } => "browser_set_viewport",
        QaDriverActionV1::BrowserScreenshot => "browser_screenshot",
        QaDriverActionV1::BrowserScroll { .. } => "browser_scroll",
        _ => "unsupported",
    }
}

pub(super) fn action_command(action: &QaDriverActionV1) -> &'static str {
    match action {
        QaDriverActionV1::BrowserOpen { .. } => "open",
        QaDriverActionV1::BrowserClick { .. } => "click",
        QaDriverActionV1::BrowserFill { .. } => "fill",
        QaDriverActionV1::BrowserPress { .. } => "press",
        QaDriverActionV1::BrowserSelect { .. } => "select",
        QaDriverActionV1::BrowserWaitForVisible { .. } => "wait_for",
        QaDriverActionV1::BrowserAssertText { .. } | QaDriverActionV1::BrowserAssertUrl { .. } => {
            "evaluate_limited_assertions"
        }
        QaDriverActionV1::BrowserSetViewport { .. } => "set_viewport",
        QaDriverActionV1::BrowserScreenshot => "screenshot",
        QaDriverActionV1::BrowserScroll { .. } => "scroll",
        _ => "unsupported",
    }
}

pub(super) fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("browser driver report is missing {key}"))
}

pub(super) fn number(value: &Value, key: &str) -> Result<u64, String> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("browser driver report has invalid {key}"))
}

pub(super) fn number_map(value: &Map<String, Value>, key: &str) -> Result<u64, String> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("browser driver report has invalid {key}"))
}

pub(super) fn object<'a>(value: &'a Value, key: &str) -> Result<&'a Map<String, Value>, String> {
    value
        .get(key)
        .and_then(Value::as_object)
        .ok_or_else(|| format!("browser driver report has invalid {key}"))
}

pub(super) fn array<'a>(value: &'a Value, key: &str) -> Result<&'a [Value], String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| format!("browser driver report has invalid {key}"))
}

pub(super) fn string_array(value: &Value, key: &str, max: usize) -> Result<Vec<String>, String> {
    let items = array(value, key)?;
    if items.len() > max {
        return Err(format!("browser driver report exceeds the {key} limit"));
    }
    items
        .iter()
        .map(|item| {
            item.as_str()
                .map(|text| text.chars().take(4096).collect())
                .ok_or_else(|| format!("browser driver report has invalid {key} item"))
        })
        .collect()
}

pub(super) fn state_text(value: &Value, key: &str) -> Result<Option<String>, String> {
    let state = object(value, key)?;
    let url = state.get("url").and_then(Value::as_str).unwrap_or("");
    if !url.is_empty() && url != "about:blank" {
        let parsed = Url::parse(url).map_err(|_| "browser driver recorded an invalid URL")?;
        if parsed.origin().ascii_serialization() != "http://qa-preview:8080"
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err("browser driver recorded a URL outside the app-owned preview relay".into());
        }
    }
    let title = state.get("title").and_then(Value::as_str).unwrap_or("");
    let text = format!("url={url}; title={title}");
    Ok(Some(text.chars().take(4096).collect()))
}

pub(super) fn bounded_u16(value: u64) -> Result<u16, String> {
    u16::try_from(value).map_err(|_| "browser driver dimension is outside supported bounds".into())
}

pub(super) fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
