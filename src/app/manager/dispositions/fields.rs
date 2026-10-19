pub(super) fn encode(value: &str) -> Result<String, String> {
    serde_json::to_string(value)
        .map(|value| format!("json:{value}"))
        .map_err(|error| error.to_string())
}

pub(super) fn decode(value: &str) -> String {
    value
        .strip_prefix("json:")
        .and_then(|encoded| serde_json::from_str(encoded).ok())
        .unwrap_or_else(|| value.to_owned())
}

#[cfg(test)]
#[path = "fields/tests.rs"]
mod tests;
