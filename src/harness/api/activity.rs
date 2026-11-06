use serde::{Deserialize, Serialize};
mod sanitize;
use sanitize::bounded_activity_text;
pub use sanitize::safe_activity_text;
mod stream;

/// Display snapshot; task snapshots are persisted privately, never as planning artifacts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LiveProgress {
    /// Stable local task and worker-run identities for this retained history.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_uid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default)]
    pub run_attempt: u32,
    /// Versioned, bounded operator-visible history reconstructed from provider snapshots.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub activity_events: Vec<TaskActivityEventV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity_events_truncated_before_ms: Option<i64>,
    pub telemetry: ActivityTelemetry,
    /// Versioned prompt identity associated with this run's activity evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_manifest: Option<crate::harness::prompt_catalog::PromptManifest>,
    /// Selected local harness/model at the start of this unit of work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_route: Option<String>,
    /// Completed provider/model responses observed during this harness run.
    /// Entries are keyed by call id so repeated snapshots replace prior data.
    #[serde(default)]
    pub model_calls: Vec<ModelCallUsage>,
    /// Zero-based acceptance-criterion indexes reported complete by the task worker.
    /// This is a full snapshot and is persisted with the task activity.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checklist: Vec<usize>,
    #[serde(default)]
    pub checklist_revision: u64,
    /// Structured dependency requests surfaced by a bounded implementation run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependency_requests: Vec<super::dependency::DependencyRequest>,
    pub posts: Vec<LivePost>,
    pub thoughts: String,
    pub response: String,
    pub specification: Option<String>,
    pub activity: Option<String>,
}

/// Provider-neutral usage from one completed model response.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ModelCallUsage {
    pub call_id: String,
    pub provider: Option<String>,
    pub api: Option<String>,
    pub model: Option<String>,
    pub requested_model: Option<String>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    /// Provider-reported estimated cost in USD micros.
    pub estimated_cost_usd_micros: Option<u64>,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub ended_at: Option<chrono::DateTime<chrono::Utc>>,
    pub duration_millis: Option<u64>,
    pub stop_reason: Option<String>,
}

/// Observed stream updates, not estimated token counts. Persisted with task activity.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ActivityTelemetry {
    pub started_ms: Option<i64>,
    pub updated_ms: Option<i64>,
    pub finished_ms: Option<i64>,
    pub updates: u64,
    /// Ten-second buckets: UTC bucket number and received update count.
    pub samples: Vec<(i64, u64)>,
    /// Versioned clean-launch evidence. Contains no argv, paths, prompts, or credentials.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch_profile: Option<CleanLaunchTelemetry>,
}

/// Redacted indication of which local authentication source was selected.
/// It never contains credential values or file paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderAuthStrategy {
    Environment,
    Saved,
    Unavailable,
}

/// Redacted evidence for the provider profile validated before process launch.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CleanLaunchTelemetry {
    pub provider: String,
    pub cli_version: String,
    pub execution_profile: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_role_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_strategy: Option<ProviderAuthStrategy>,
    pub profile_hash: String,
    pub verified_capabilities: Vec<String>,
}

/// A stable, chronologically placed block of external agent output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LivePost {
    pub id: (u64, usize),
    pub kind: String,
    pub text: String,
    /// First application observation time, stable while provider snapshots coalesce.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recorded_at_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskActivityEventV1 {
    pub schema_version: u16,
    pub event_id: String,
    /// Monotonic insertion sequence within the retained task history.
    #[serde(default)]
    pub sequence: u64,
    pub task_uid: String,
    pub run_id: String,
    pub attempt: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    pub recorded_at_ms: i64,
    pub actor: TaskActivityActor,
    pub origin: TaskActivityOrigin,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    pub content: String,
    pub visibility: TaskActivityVisibility,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskActivityActor {
    Worker,
    System,
    Reviewer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskActivityVisibility {
    UserSafe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskActivityOrigin {
    ProviderObserved,
    ApplicationInferred,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_route_survives_provider_progress_snapshots() {
        let mut progress = LiveProgress {
            selected_route: Some("codex / gpt-configured".into()),
            ..Default::default()
        };
        progress.update(LiveProgress {
            activity: Some("Reading the task".into()),
            ..Default::default()
        });
        assert_eq!(
            progress.selected_route.as_deref(),
            Some("codex / gpt-configured")
        );
        assert_eq!(progress.activity.as_deref(), Some("Reading the task"));
    }

    #[test]
    fn redacted_launch_profile_survives_later_progress_updates() {
        let profile = CleanLaunchTelemetry {
            provider: "codex".into(),
            cli_version: "codex 1.2.3".into(),
            execution_profile: "manager-cli-v1".into(),
            prompt_role_version: None,
            auth_strategy: Some(ProviderAuthStrategy::Saved),
            profile_hash: "sha256:synthetic".into(),
            verified_capabilities: vec!["--ignore-user-config".into()],
        };
        let encoded = serde_json::to_string(&profile).unwrap();
        assert!(encoded.contains("\"authStrategy\":\"saved\""));
        assert!(!encoded.contains("auth.json"));
        assert!(!encoded.contains("credential"));
        let mut progress = LiveProgress {
            telemetry: ActivityTelemetry {
                launch_profile: Some(profile.clone()),
                ..Default::default()
            },
            ..Default::default()
        };
        progress.update(LiveProgress {
            activity: Some("Reading the task".into()),
            ..Default::default()
        });
        assert_eq!(progress.telemetry.launch_profile, Some(profile));
        let serialized = serde_json::to_string(&progress).unwrap();
        assert!(!serialized.contains("argv"));
        assert!(!serialized.contains("credential"));
    }
}
