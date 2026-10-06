use std::{path::PathBuf, sync::mpsc, time::Instant};

use crate::{
    artifacts::task_docs::TaskMetadata,
    harness::{AiHarness, HarnessOutcome, LiveProgress, PlanningRequest, RetrievalPlan},
    persistence::{project_slug, telemetry::InvocationRecord},
};

/// Captures one task's implementation-related harness invocations while
/// preserving the ordinary harness and progress contracts.
pub(super) struct CaptureHarness<'a> {
    inner: &'a dyn AiHarness,
    project_root: PathBuf,
    repository: Option<String>,
    feature: Option<String>,
    batch: Option<String>,
    task: Option<String>,
    pub(super) session: String,
}

impl<'a> CaptureHarness<'a> {
    pub(super) fn new(
        inner: &'a dyn AiHarness,
        project_root: &std::path::Path,
        ticket_text: &str,
        metadata: Option<&TaskMetadata>,
        task_uid: Option<&str>,
    ) -> Self {
        let feature = ticket_text
            .lines()
            .find_map(|line| line.strip_prefix("Feature: "))
            .and_then(|value| {
                crate::core::workflow::feature_ids_in(value)
                    .into_iter()
                    .next()
            });
        Self {
            inner,
            project_root: project_root.to_path_buf(),
            repository: metadata.map(|metadata| metadata.repository_id.clone()),
            feature,
            batch: metadata.map(|metadata| metadata.batch_uid.clone()),
            task: task_uid.map(str::to_owned),
            session: format!("session-{}", uuid::Uuid::new_v4()),
        }
    }

    fn persist(
        &self,
        request: &PlanningRequest,
        start: Instant,
        progress: Option<LiveProgress>,
        outcome: &str,
    ) -> Option<String> {
        let project = self
            .project_root
            .canonicalize()
            .unwrap_or_else(|_| self.project_root.clone());
        let slug = project_slug(&project);
        let end = chrono::Utc::now();
        let start_at = end - chrono::Duration::from_std(start.elapsed()).unwrap_or_default();
        let calls = progress
            .map(|progress| progress.model_calls)
            .unwrap_or_default();
        let harness = self.inner.label();
        let (name, version) = split_harness(&harness);
        let phase = request
            .telemetry_phase
            .clone()
            .unwrap_or_else(|| match request.mode {
                crate::harness::ExecutionMode::Implementation => "implementation".into(),
                crate::harness::ExecutionMode::Reconciliation => "reconciliation".into(),
                crate::harness::ExecutionMode::ReadOnlyAnalysis => "verification".into(),
                _ => "other".into(),
            });
        let calls = if calls.is_empty() {
            vec![None]
        } else {
            calls.into_iter().map(Some).collect()
        };
        let mut errors = Vec::new();
        for call in calls {
            let mut record = InvocationRecord::new(&slug, name);
            record.session = Some(self.session.clone());
            record.repository = self.repository.clone();
            record.feature = self.feature.clone();
            record.batch = self.batch.clone();
            record.task = self.task.clone();
            record.phase = Some(phase.clone());
            record.harness_version = version.clone();
            record.thinking_level = Some(request.reasoning_level.clone());
            record.started_at = call
                .as_ref()
                .and_then(|call| call.started_at)
                .or(Some(start_at));
            record.ended_at = call.as_ref().and_then(|call| call.ended_at).or(Some(end));
            record.duration_millis = call
                .as_ref()
                .and_then(|call| call.duration_millis)
                .or_else(|| Some(start.elapsed().as_millis().min(u128::from(u64::MAX)) as u64));
            record.outcome = outcome.into();
            if call.is_some() {
                record.model_calls = Some(1);
                record.provider = call.as_ref().and_then(|call| call.provider.clone());
                record.api = call.as_ref().and_then(|call| call.api.clone());
                record.model = call.as_ref().and_then(|call| call.model.clone());
                record.requested_model =
                    call.as_ref().and_then(|call| call.requested_model.clone());
                record.input_tokens = call.as_ref().and_then(|call| call.input_tokens);
                record.output_tokens = call.as_ref().and_then(|call| call.output_tokens);
                record.cache_read_tokens = call.as_ref().and_then(|call| call.cache_read_tokens);
                record.cache_write_tokens = call.as_ref().and_then(|call| call.cache_write_tokens);
                record.reasoning_tokens = call.as_ref().and_then(|call| call.reasoning_tokens);
                record.total_tokens = call.as_ref().and_then(|call| call.total_tokens);
                record.estimated_cost_usd_micros = call
                    .as_ref()
                    .and_then(|call| call.estimated_cost_usd_micros);
                record.estimated_cost_usd_cents = record
                    .estimated_cost_usd_micros
                    .map(|micros| micros.saturating_add(5_000) / 10_000);
                record.provider_stop_reason =
                    call.as_ref().and_then(|call| call.stop_reason.clone());
            }
            if let Err(error) = crate::persistence::telemetry::append(&slug, &record) {
                errors.push(error.to_string());
            }
        }
        (!errors.is_empty()).then(|| errors.join("; "))
    }
}

/// Records Kool.ad/e's active implementation span independently from model
/// calls, even when no provider usage is available or a run ends with error.
pub(super) struct ActiveSpan {
    record: InvocationRecord,
    started: Instant,
    progress: std::sync::mpsc::Sender<LiveProgress>,
}

impl ActiveSpan {
    pub(super) fn new(
        project_root: &std::path::Path,
        ticket_text: &str,
        metadata: Option<&TaskMetadata>,
        task_uid: Option<&str>,
        session: &str,
        progress: std::sync::mpsc::Sender<LiveProgress>,
    ) -> Self {
        let project = project_root
            .canonicalize()
            .unwrap_or_else(|_| project_root.to_path_buf());
        let mut record = InvocationRecord::new(project_slug(&project), "Kool.ad/e");
        record.session = Some(session.to_owned());
        record.repository = metadata.map(|metadata| metadata.repository_id.clone());
        record.feature = ticket_text
            .lines()
            .find_map(|line| line.strip_prefix("Feature: "))
            .and_then(|value| {
                crate::core::workflow::feature_ids_in(value)
                    .into_iter()
                    .next()
            });
        record.batch = metadata.map(|metadata| metadata.batch_uid.clone());
        record.task = task_uid.map(str::to_owned);
        record.phase = Some("active_implementation".into());
        record.model_calls = Some(0);
        Self {
            record,
            started: Instant::now(),
            progress,
        }
    }
}

impl Drop for ActiveSpan {
    fn drop(&mut self) {
        let ended_at = chrono::Utc::now();
        let duration = self.started.elapsed();
        self.record.started_at =
            Some(ended_at - chrono::Duration::from_std(duration).unwrap_or_default());
        self.record.ended_at = Some(ended_at);
        self.record.duration_millis = Some(duration.as_millis().min(u128::from(u64::MAX)) as u64);
        self.record.outcome = "ended".into();
        let slug = self.record.project.clone();
        if let Err(error) = crate::persistence::telemetry::append(&slug, &self.record) {
            let _ = self.progress.send(LiveProgress {
                activity: Some(format!("Active-time telemetry could not be saved: {error}. Work continues; metrics may be incomplete.")),
                ..Default::default()
            });
        }
    }
}

#[cfg(test)]
#[path = "telemetry/tests.rs"]
mod tests;

impl AiHarness for CaptureHarness<'_> {
    fn label(&self) -> String {
        self.inner.label()
    }
    fn check_available(&self) -> Result<String, crate::error::AppError> {
        self.inner.check_available()
    }
    fn plan_retrieval(
        &self,
        request: &PlanningRequest,
    ) -> Result<Option<RetrievalPlan>, crate::error::AppError> {
        self.inner.plan_retrieval(request)
    }

    fn execute(&self, request: &PlanningRequest) -> Result<HarnessOutcome, crate::error::AppError> {
        let start = Instant::now();
        let (tx, rx) = mpsc::channel();
        let mut captured = request.clone();
        captured.progress_tx = tx;
        let (execution, latest) = std::thread::scope(|scope| {
            let target = request.progress_tx.clone();
            let reader = scope.spawn(move || {
                let mut aggregate = LiveProgress::default();
                let mut saw_progress = false;
                while let Ok(progress) = rx.recv() {
                    let _ = target.send(progress.clone());
                    aggregate.update(progress);
                    saw_progress = true;
                }
                saw_progress.then_some(aggregate)
            });
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.inner.execute(&captured)
            }));
            drop(captured);
            (result, reader.join().ok().flatten())
        });
        let outcome = match &execution {
            Ok(result) => invocation_outcome(result),
            Err(_) => "interrupted",
        };
        if let Some(error) = self.persist(request, start, latest.clone(), outcome) {
            let mut progress = latest.unwrap_or_default();
            progress.activity = Some(format!(
                "Implementation telemetry could not be saved: {error}. Work continues; metrics may be incomplete."
            ));
            let _ = request.progress_tx.send(progress);
        }
        match execution {
            Ok(result) => result,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }
}

fn invocation_outcome(result: &Result<HarnessOutcome, crate::error::AppError>) -> &'static str {
    match result {
        Ok(_) => "completed",
        Err(crate::error::AppError::HarnessTimedOut { .. }) => "timed-out",
        Err(crate::error::AppError::HarnessFailed { reason, .. })
            if reason.to_ascii_lowercase().contains("cancel") =>
        {
            "cancelled"
        }
        Err(crate::error::AppError::Other(reason))
            if reason.to_ascii_lowercase().contains("needs attention") =>
        {
            "user-action-required"
        }
        Err(_) => "failed",
    }
}

fn split_harness(label: &str) -> (&str, Option<String>) {
    label
        .split_once(' ')
        .map_or((label, None), |(name, version)| {
            (name, Some(version.into()))
        })
}
