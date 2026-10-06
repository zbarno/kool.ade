//! Version-tagged invocation record — the durable telemetry line format.
//!
//! One record describes one AI/harness invocation (F11 spec R3/R4, issue #23):
//! who ran it (attribution identity), what it used (per-category tokens and
//! model-call count), how long it took (monotonic duration plus wall-clock
//! timestamps), what it cost (stored estimate tagged with its price-table
//! version), and how it ended (outcome).
//!
//! Format rules pinned by CLR-012 (F11): one JSON object per line, each line
//! carrying its own `schemaVersion` so readers can skip versions they do not
//! understand instead of rejecting the file, and provider-specific fields are
//! nullable — a missing field degrades to `null` instead of dropping the rest
//! of the record (R4, AC21).
//!
//! Cost: the store keeps the raw tokens, the originating `price_table_version`
//! and the estimate computed at creation time; viewers must never recompute
//! from a later table (R6, AC20). Unknown models stay `null` (rendered as
//! "n/a"), never a fabricated zero.

use chrono::DateTime;
use serde::{Deserialize, Serialize};

/// `schemaVersion` written by, and understood by, this reader/writer.
pub const SCHEMA_VERSION: u32 = 1;

/// Placeholder for an invocation whose ending has not been classified yet.
fn default_outcome() -> String {
    "unknown".to_owned()
}

/// One completed (or failed/interrupted) AI/harness invocation.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InvocationRecord {
    /// Format tag. Absent or unparsable lines must not decode.
    pub schema_version: u32,

    // Attribution (R3).
    pub project: String,
    #[serde(default)]
    pub session: Option<String>,
    #[serde(default)]
    pub repository: Option<String>,
    #[serde(default)]
    pub feature: Option<String>,
    #[serde(default)]
    pub batch: Option<String>,
    #[serde(default)]
    pub task: Option<String>,
    /// Implementation phase, e.g. "implementation", "reconciliation",
    /// "repair" — free-form so future phases need no format bump.
    #[serde(default)]
    pub phase: Option<String>,
    pub harness: String,
    #[serde(default)]
    pub harness_version: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub api: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    /// Requested model, kept distinct from the responded-with model (AC16).
    #[serde(default)]
    pub requested_model: Option<String>,
    /// Reasoning/thinking level when the harness exposes one (AC17).
    #[serde(default)]
    pub thinking_level: Option<String>,

    // Usage (R4, AC10-AC13). Categories are stored independently; a category
    // the provider did not report stays `null` rather than pretending zero,
    // and reasoning tokens that the provider folds into output are not
    // duplicated.
    #[serde(default)]
    pub model_calls: Option<u32>,
    #[serde(default)]
    pub input_tokens: Option<u64>,
    #[serde(default)]
    pub output_tokens: Option<u64>,
    #[serde(default)]
    pub cache_read_tokens: Option<u64>,
    #[serde(default)]
    pub cache_write_tokens: Option<u64>,
    #[serde(default)]
    pub reasoning_tokens: Option<u64>,
    #[serde(default)]
    pub total_tokens: Option<u64>,

    // Duration: monotonic measurement where practical, wall-clock RFC-3339
    // timestamps for timeline placement.
    #[serde(default)]
    pub duration_millis: Option<u64>,
    #[serde(default)]
    pub started_at: Option<DateTime<chrono::Utc>>,
    #[serde(default)]
    pub ended_at: Option<DateTime<chrono::Utc>>,

    // Cost: the stored estimate plus the price-table version that produced it.
    #[serde(default)]
    pub estimated_cost_usd_cents: Option<u64>,
    #[serde(default)]
    pub price_table_version: Option<String>,

    /// How the invocation ended. Conventionally "completed", "failed",
    /// "timed-out" or "cancelled", plus provider stop reasons; free-form so
    /// the record stays harness-neutral.
    #[serde(default = "default_outcome")]
    pub outcome: String,
}

impl InvocationRecord {
    /// Fresh record with the current schema tag. Attribution, usage, timing
    /// and cost stay unset (`null`) until the harness supplies them.
    pub fn new(project: impl Into<String>, harness: impl Into<String>) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            project: project.into(),
            harness: harness.into(),
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fully_populated() -> InvocationRecord {
        let mut r = InvocationRecord::new("acct-project", "pi");
        r.session = Some("sess-7".into());
        r.repository = Some("zbarno/kool.ade".into());
        r.feature = Some("F11".into());
        r.batch = Some("F11-02".into());
        r.task = Some("F11-TASK-xyz".into());
        r.phase = Some("implementation".into());
        r.harness_version = Some("pi-0.20.3".into());
        r.provider = Some("anthropic".into());
        r.api = Some("messages".into());
        r.model = Some("claude-sonnet-4-5".into());
        r.requested_model = Some("claude-sonnet-4-5".into());
        r.thinking_level = Some("medium".into());
        r.model_calls = Some(17);
        r.input_tokens = Some(842_119);
        r.output_tokens = Some(29_841);
        r.cache_read_tokens = Some(691_440);
        r.cache_write_tokens = Some(38_212);
        r.reasoning_tokens = Some(18_104);
        r.total_tokens = Some(910_172);
        r.duration_millis = Some(711_000);
        r.started_at = DateTime::from_timestamp_millis(1_760_000_000_000);
        r.ended_at = DateTime::from_timestamp_millis(1_760_000_711_000);
        r.estimated_cost_usd_cents = Some(184);
        r.price_table_version = Some("price_table_v1_2025Q3".into());
        r.outcome = "completed".into();
        r
    }

    #[test]
    fn round_trip_preserves_every_field_with_contract_names() {
        let original = fully_populated();
        let line = serde_json::to_string(&original).unwrap();
        // Pin the wire contract: camelCase tags a reader can rely on.
        assert!(line.contains("\"schemaVersion\":1"));
        assert!(line.contains("\"cacheReadTokens\":691440"));
        assert!(line.contains("\"durationMillis\":711000"));
        assert!(line.contains("\"priceTableVersion\":\"price_table_v1_2025Q3\""));
        let decoded: InvocationRecord = serde_json::from_str(&line).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn minimal_and_null_fields_degrade_gracefully() {
        let minimal: InvocationRecord =
            serde_json::from_str(r#"{"schemaVersion":1,"project":"p","harness":"pi"}"#).unwrap();
        assert!(minimal.total_tokens.is_none());
        assert_eq!(minimal.outcome, "unknown");

        let nulled: InvocationRecord = serde_json::from_str(
            r#"{"schemaVersion":1,"project":"p","harness":"pi","provider":null,
                 "reasoningTokens":null,"estimatedCostUsdCents":null}"#,
        )
        .unwrap();
        assert_eq!(nulled.provider, None);
        assert_eq!(nulled.reasoning_tokens, None);
        assert_eq!(nulled.estimated_cost_usd_cents, None);
    }

    #[test]
    fn record_without_a_schema_tag_does_not_decode() {
        let err = serde_json::from_str::<InvocationRecord>(r#"{"project":"p"}"#);
        assert!(err.is_err(), "an untagged line is not a valid record");
    }
}
