use serde::{Deserialize, Serialize};

/// Display snapshot; task snapshots are persisted privately, never as planning artifacts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LiveProgress {
    pub telemetry: ActivityTelemetry,
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
}

/// A stable, chronologically placed block of external agent output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LivePost {
    pub id: (u64, usize),
    pub kind: String,
    pub text: String,
}

impl LiveProgress {
    pub fn update(&mut self, mut next: Self) {
        // Progress snapshots can be delayed behind a newer snapshot. Checklist
        // reports carry a worker-local revision so late snapshots cannot roll
        // the durable board state back.
        if next.checklist_revision < self.checklist_revision {
            next.checklist = std::mem::take(&mut self.checklist);
            next.checklist_revision = self.checklist_revision;
        }
        for call in next.model_calls.drain(..) {
            if let Some(existing) = self
                .model_calls
                .iter_mut()
                .find(|item| item.call_id == call.call_id)
            {
                *existing = merge_model_call(call, existing);
            } else {
                self.model_calls.push(call);
            }
        }
        next.model_calls = std::mem::take(&mut self.model_calls);
        for post in next.posts.drain(..) {
            if let Some(existing) = self.posts.iter_mut().find(|p| p.id == post.id) {
                *existing = post;
            } else {
                self.posts.push(post);
            }
        }
        next.posts = std::mem::take(&mut self.posts);
        let now = chrono::Utc::now().timestamp_millis();
        let mut telemetry = std::mem::take(&mut self.telemetry);
        telemetry.started_ms.get_or_insert(now);
        telemetry.updated_ms = Some(now);
        telemetry.updates += 1;
        let bucket = now / 10_000;
        if let Some((_, count)) = telemetry
            .samples
            .last_mut()
            .filter(|(last, _)| *last == bucket)
        {
            *count += 1;
        } else {
            telemetry.samples.push((bucket, 1));
        }
        telemetry.samples.retain(|(time, _)| *time >= bucket - 59);
        next.telemetry = telemetry;
        next.selected_route = next.selected_route.or_else(|| self.selected_route.clone());
        *self = next;
    }
}

fn merge_model_call(mut next: ModelCallUsage, prior: &ModelCallUsage) -> ModelCallUsage {
    next.provider = next.provider.or_else(|| prior.provider.clone());
    next.api = next.api.or_else(|| prior.api.clone());
    next.model = next.model.or_else(|| prior.model.clone());
    next.requested_model = next
        .requested_model
        .or_else(|| prior.requested_model.clone());
    next.input_tokens = next.input_tokens.or(prior.input_tokens);
    next.output_tokens = next.output_tokens.or(prior.output_tokens);
    next.cache_read_tokens = next.cache_read_tokens.or(prior.cache_read_tokens);
    next.cache_write_tokens = next.cache_write_tokens.or(prior.cache_write_tokens);
    next.reasoning_tokens = next.reasoning_tokens.or(prior.reasoning_tokens);
    next.total_tokens = next.total_tokens.or(prior.total_tokens);
    next.estimated_cost_usd_micros = next
        .estimated_cost_usd_micros
        .or(prior.estimated_cost_usd_micros);
    next.started_at = next.started_at.or(prior.started_at);
    next.ended_at = next.ended_at.or(prior.ended_at);
    next.duration_millis = next.duration_millis.or(prior.duration_millis);
    next.stop_reason = next.stop_reason.or_else(|| prior.stop_reason.clone());
    next
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
}
