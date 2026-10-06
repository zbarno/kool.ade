use std::collections::BTreeMap;

use chrono::{DateTime, Utc};

use super::InvocationRecord;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ModelIdentity {
    harness: String,
    provider: Option<String>,
    api: Option<String>,
    model: Option<String>,
    requested_model: Option<String>,
    thinking_level: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MetricBreakdown {
    pub label: String,
    pub harness: Option<String>,
    pub provider: Option<String>,
    pub api: Option<String>,
    pub model: Option<String>,
    pub requested_model: Option<String>,
    pub thinking_level: Option<String>,
    pub duration_millis: u64,
    pub model_calls: Option<u64>,
    pub total_tokens: Option<u64>,
    pub estimated_cost_usd_micros: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImplementationMetrics {
    pub elapsed_millis: Option<u64>,
    pub active_implementation_millis: u64,
    pub ai_execution_millis: u64,
    pub model_calls: Option<u64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub estimated_cost_usd_micros: Option<u64>,
    pub by_harness_model: Vec<MetricBreakdown>,
    pub by_phase: Vec<MetricBreakdown>,
}

pub fn for_task(records: &[InvocationRecord], task: &str) -> ImplementationMetrics {
    aggregate(
        records
            .iter()
            .filter(|record| record.task.as_deref() == Some(task)),
    )
}

pub fn for_feature(records: &[InvocationRecord], feature: &str) -> ImplementationMetrics {
    aggregate(
        records
            .iter()
            .filter(|record| record.feature.as_deref() == Some(feature)),
    )
}

fn aggregate<'a>(records: impl Iterator<Item = &'a InvocationRecord>) -> ImplementationMetrics {
    let records = records.collect::<Vec<_>>();
    let mut result = ImplementationMetrics::default();
    let mut first = None::<DateTime<Utc>>;
    let mut last = None::<DateTime<Utc>>;
    let mut known = [None::<u64>; 7];
    let mut model = BTreeMap::<ModelIdentity, MetricBreakdown>::new();
    let mut phase = BTreeMap::<String, MetricBreakdown>::new();
    let mut elapsed_ai = 0u64;
    for record in records {
        if let Some(start) = record.started_at {
            first = Some(first.map_or(start, |prior| prior.min(start)));
        }
        if let Some(end) = record.ended_at {
            last = Some(last.map_or(end, |prior| prior.max(end)));
        }
        let duration = record.duration_millis.unwrap_or_default();
        if record.phase.as_deref() == Some("active_implementation") {
            result.active_implementation_millis =
                result.active_implementation_millis.saturating_add(duration);
            let label = "active_implementation".to_owned();
            let item = phase
                .entry(label.clone())
                .or_insert_with(|| MetricBreakdown {
                    label,
                    ..Default::default()
                });
            item.duration_millis = item.duration_millis.saturating_add(duration);
            continue;
        }
        elapsed_ai = elapsed_ai.saturating_add(duration);
        if let Some(calls) = record.model_calls {
            result.model_calls = Some(
                result
                    .model_calls
                    .unwrap_or_default()
                    .saturating_add(u64::from(calls)),
            );
        }
        for (slot, value) in known.iter_mut().zip([
            record.input_tokens,
            record.output_tokens,
            record.cache_read_tokens,
            record.cache_write_tokens,
            record.reasoning_tokens,
            record.total_tokens,
            record.estimated_cost_usd_micros.or_else(|| {
                record
                    .estimated_cost_usd_cents
                    .map(|c| c.saturating_mul(10_000))
            }),
        ]) {
            if let Some(value) = value {
                *slot = Some(slot.unwrap_or_default().saturating_add(value));
            }
        }
        let identity = ModelIdentity {
            harness: record.harness.clone(),
            provider: record.provider.clone(),
            api: record.api.clone(),
            model: record.model.clone(),
            requested_model: record.requested_model.clone(),
            thinking_level: record.thinking_level.clone(),
        };
        let label = model_label(&identity);
        let item = model
            .entry(identity.clone())
            .or_insert_with(|| MetricBreakdown {
                label,
                harness: Some(identity.harness.clone()),
                provider: identity.provider.clone(),
                api: identity.api.clone(),
                model: identity.model.clone(),
                requested_model: identity.requested_model.clone(),
                thinking_level: identity.thinking_level.clone(),
                ..Default::default()
            });
        add_breakdown(item, record, duration);
        let label = record.phase.clone().unwrap_or_else(|| "unknown".into());
        let item = phase
            .entry(label.clone())
            .or_insert_with(|| MetricBreakdown {
                label,
                ..Default::default()
            });
        add_breakdown(item, record, duration);
    }
    result.elapsed_millis = first
        .zip(last)
        .map(|(start, end)| (end - start).num_milliseconds().max(0) as u64);
    result.ai_execution_millis = elapsed_ai;
    result.input_tokens = known[0];
    result.output_tokens = known[1];
    result.cache_read_tokens = known[2];
    result.cache_write_tokens = known[3];
    result.reasoning_tokens = known[4];
    result.total_tokens = known[5];
    result.estimated_cost_usd_micros = known[6];
    result.by_harness_model = model.into_values().collect();
    result.by_phase = phase.into_values().collect();
    result
}

fn model_label(identity: &ModelIdentity) -> String {
    let mut parts = vec![identity.harness.as_str()];
    parts.extend(identity.provider.as_deref());
    parts.extend(identity.api.as_deref());
    parts.extend(identity.model.as_deref());
    if let Some(requested) = identity.requested_model.as_deref() {
        parts.push("requested");
        parts.push(requested);
    }
    if let Some(thinking) = identity.thinking_level.as_deref() {
        parts.push("thinking");
        parts.push(thinking);
    }
    parts.join(" · ")
}

fn add_breakdown(item: &mut MetricBreakdown, record: &InvocationRecord, duration: u64) {
    if record.model_calls == Some(0) {
        return;
    }
    item.duration_millis = item.duration_millis.saturating_add(duration);
    if let Some(calls) = record.model_calls {
        item.model_calls = Some(
            item.model_calls
                .unwrap_or_default()
                .saturating_add(u64::from(calls)),
        );
    }
    if let Some(tokens) = record.total_tokens {
        item.total_tokens = Some(item.total_tokens.unwrap_or_default().saturating_add(tokens));
    }
    if let Some(cost) = record.estimated_cost_usd_micros.or_else(|| {
        record
            .estimated_cost_usd_cents
            .map(|c| c.saturating_mul(10_000))
    }) {
        item.estimated_cost_usd_micros = Some(
            item.estimated_cost_usd_micros
                .unwrap_or_default()
                .saturating_add(cost),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(
        task: &str,
        feature: &str,
        phase: &str,
        started: i64,
        duration: u64,
    ) -> InvocationRecord {
        let mut record = InvocationRecord::new("project", "pi");
        record.task = Some(task.into());
        record.feature = Some(feature.into());
        record.phase = Some(phase.into());
        record.started_at = DateTime::from_timestamp_millis(started);
        record.ended_at = DateTime::from_timestamp_millis(started + duration as i64);
        record.duration_millis = Some(duration);
        record.model_calls = Some(1);
        record.input_tokens = Some(10);
        record.output_tokens = Some(5);
        record.total_tokens = Some(15);
        record.estimated_cost_usd_micros = Some(125);
        record
    }

    #[test]
    fn task_and_feature_rollups_reconcile_and_keep_active_time_separate() {
        let mut active = row("t1", "F1", "active_implementation", 100, 80);
        active.model_calls = Some(0);
        let records = vec![
            row("t1", "F1", "implementation", 100, 40),
            active,
            row("t2", "F1", "repair", 200, 60),
            row("t3", "F2", "implementation", 100, 999),
        ];
        let task = for_task(&records, "t1");
        let feature = for_feature(&records, "F1");
        assert_eq!(task.active_implementation_millis, 80);
        assert_eq!(task.model_calls, Some(1));
        assert_eq!(task.input_tokens, Some(10));
        assert_eq!(feature.model_calls, Some(2));
        assert_eq!(feature.input_tokens, Some(20));
        assert_eq!(feature.estimated_cost_usd_micros, Some(250));
    }

    #[test]
    fn missing_usage_stays_unknown_instead_of_zero() {
        let mut record = row("t1", "F1", "implementation", 100, 40);
        record.input_tokens = None;
        record.output_tokens = None;
        record.total_tokens = None;
        record.estimated_cost_usd_micros = None;
        record.estimated_cost_usd_cents = None;
        let report = for_task(&[record], "t1");
        assert_eq!(report.input_tokens, None);
        assert_eq!(report.total_tokens, None);
        assert_eq!(report.estimated_cost_usd_micros, None);
    }

    #[test]
    fn provider_api_and_model_request_identity_are_kept_separate() {
        let mut first = row("t1", "F1", "implementation", 100, 40);
        first.provider = Some("provider-a".into());
        first.api = Some("messages".into());
        first.model = Some("same-model".into());
        first.requested_model = Some("alias-a".into());
        first.thinking_level = Some("high".into());
        let mut second = first.clone();
        second.provider = Some("provider-b".into());
        second.api = Some("responses".into());
        second.requested_model = Some("alias-b".into());
        let report = for_task(&[first, second], "t1");
        assert_eq!(report.by_harness_model.len(), 2);
        assert!(report.by_harness_model.iter().any(|item| {
            item.provider.as_deref() == Some("provider-a")
                && item.api.as_deref() == Some("messages")
                && item.model.as_deref() == Some("same-model")
                && item.requested_model.as_deref() == Some("alias-a")
                && item.thinking_level.as_deref() == Some("high")
        }));
    }
}
