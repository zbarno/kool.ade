use serde_json::Value;

use super::super::ModelCallUsage;

pub(super) fn call_id(message: Option<&Value>, fallback: usize) -> String {
    message
        .and_then(|value| value.get("id"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("response-{fallback}"))
}

pub(super) fn parse(message: Option<&Value>, call_id: String) -> ModelCallUsage {
    let mut call = ModelCallUsage {
        call_id,
        ..Default::default()
    };
    let Some(message) = message else { return call };
    call.provider = text(message, &["provider"]);
    call.api = text(message, &["api"]);
    call.model = text(message, &["model"]);
    call.requested_model = text(message, &["requestedModel", "requested_model"]);
    let usage = message.get("usage").unwrap_or(&Value::Null);
    call.input_tokens = number(usage, &["input", "inputTokens", "input_tokens"]);
    call.output_tokens = number(usage, &["output", "outputTokens", "output_tokens"]);
    call.cache_read_tokens = number(usage, &["cacheRead", "cacheReadTokens", "cache_read"]);
    call.cache_write_tokens = number(usage, &["cacheWrite", "cacheWriteTokens", "cache_write"]);
    call.reasoning_tokens = number(usage, &["reasoning", "reasoningTokens", "reasoning_tokens"]);
    call.total_tokens = number(usage, &["totalTokens", "total_tokens"]);
    let cost = usage.get("cost").unwrap_or(&Value::Null);
    call.estimated_cost_usd_micros = cost
        .get("total")
        .and_then(Value::as_f64)
        .filter(|amount| amount.is_finite() && *amount >= 0.0)
        .map(|amount| (amount * 1_000_000.0).round().min(u64::MAX as f64) as u64);
    call
}

pub(super) fn parse_update(event: &Value, call_id: String) -> ModelCallUsage {
    let mut call = parse(event.get("message"), call_id);
    let usage = event.get("usage").unwrap_or(&Value::Null);
    call.input_tokens = number(usage, &["input", "inputTokens", "input_tokens"]);
    call.output_tokens = number(usage, &["output", "outputTokens", "output_tokens"]);
    call.cache_read_tokens = number(usage, &["cacheRead", "cacheReadTokens", "cache_read"]);
    call.cache_write_tokens = number(usage, &["cacheWrite", "cacheWriteTokens", "cache_write"]);
    call.reasoning_tokens = number(usage, &["reasoning", "reasoningTokens", "reasoning_tokens"]);
    call.total_tokens = number(usage, &["totalTokens", "total_tokens"]);
    let cost = usage.get("cost").unwrap_or(&Value::Null);
    call.estimated_cost_usd_micros = cost
        .get("total")
        .and_then(Value::as_f64)
        .filter(|amount| amount.is_finite() && *amount >= 0.0)
        .map(|amount| (amount * 1_000_000.0).round().min(u64::MAX as f64) as u64);
    call
}

pub(super) fn merge(mut later: ModelCallUsage, earlier: &ModelCallUsage) -> ModelCallUsage {
    later.provider = later.provider.or_else(|| earlier.provider.clone());
    later.api = later.api.or_else(|| earlier.api.clone());
    later.model = later.model.or_else(|| earlier.model.clone());
    later.requested_model = later
        .requested_model
        .or_else(|| earlier.requested_model.clone());
    later.input_tokens = later.input_tokens.or(earlier.input_tokens);
    later.output_tokens = later.output_tokens.or(earlier.output_tokens);
    later.cache_read_tokens = later.cache_read_tokens.or(earlier.cache_read_tokens);
    later.cache_write_tokens = later.cache_write_tokens.or(earlier.cache_write_tokens);
    later.reasoning_tokens = later.reasoning_tokens.or(earlier.reasoning_tokens);
    later.total_tokens = later.total_tokens.or(earlier.total_tokens);
    later.estimated_cost_usd_micros = later
        .estimated_cost_usd_micros
        .or(earlier.estimated_cost_usd_micros);
    later.started_at = later.started_at.or(earlier.started_at);
    later.duration_millis = later.duration_millis.or(earlier.duration_millis);
    later
}

fn text(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

fn number(value: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_fields_are_normalized_without_inventing_missing_values() {
        let message = serde_json::json!({
            "id": "response-1", "provider": "anthropic", "api": "messages",
            "model": "claude-sonnet", "usage": {
                "input": 12, "output": 4, "cacheRead": 8, "cacheWrite": 2,
                "reasoning": 3, "totalTokens": 26, "cost": {"total": 0.001234}
            }
        });
        let call = parse(Some(&message), "response-1".into());
        assert_eq!(call.provider.as_deref(), Some("anthropic"));
        assert_eq!(call.input_tokens, Some(12));
        assert_eq!(call.reasoning_tokens, Some(3));
        assert_eq!(call.estimated_cost_usd_micros, Some(1234));
        assert_eq!(call.started_at, None);
    }

    #[test]
    fn absent_usage_still_records_a_model_call_without_zero_filling() {
        let call = parse(
            Some(&serde_json::json!({"model":"local-model"})),
            "x".into(),
        );
        assert_eq!(call.model.as_deref(), Some("local-model"));
        assert_eq!(call.input_tokens, None);
        assert_eq!(call.estimated_cost_usd_micros, None);
    }
}
