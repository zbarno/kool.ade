use super::CaptureHarness;
use crate::harness::{HarnessOutcome, LiveProgress, PlanningRequest, ReviewExecutionIdentity};
use std::{sync::mpsc, time::Instant};

pub(super) fn execute(
    capture: &CaptureHarness<'_>,
    request: &PlanningRequest,
    identity: Option<&ReviewExecutionIdentity>,
) -> Result<HarnessOutcome, crate::error::AppError> {
    let start = Instant::now();
    let (tx, rx) = mpsc::channel();
    let mut captured = request.clone();
    captured.progress_tx = tx;
    if let Some(identity) = identity {
        captured.review_identity = Some(identity.clone());
    }
    if captured.mode == crate::harness::ExecutionMode::Implementation {
        captured.runtime_config_source = Some(capture.runtime_config_source.clone());
    }
    let (execution, latest) = std::thread::scope(|scope| {
        let target = request.progress_tx.clone();
        let reader = scope.spawn(move || {
            let mut aggregate = LiveProgress::default();
            let mut saw_progress = false;
            while let Ok(mut progress) = rx.recv() {
                if progress.prompt_manifest.is_none() {
                    progress.prompt_manifest = request.prompt_manifest.clone();
                }
                let _ = target.send(progress.clone());
                aggregate.update(progress);
                saw_progress = true;
            }
            saw_progress.then_some(aggregate)
        });
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if let Some(identity) = identity {
                capture.inner.execute_review(&captured, identity)
            } else {
                capture.inner.execute(&captured)
            }
        }));
        drop(captured);
        (result, reader.join().ok().flatten())
    });
    let outcome = match &execution {
        Ok(result) => super::invocation_outcome(result),
        Err(_) => "interrupted",
    };
    if let Some(error) = capture.persist(request, start, latest.clone(), outcome, identity) {
        capture
            .telemetry_complete
            .store(false, std::sync::atomic::Ordering::SeqCst);
        if identity.is_some() {
            return Err(crate::error::AppError::Other(format!(
                "Independent reviewer run identity could not be durably recorded: {error}"
            )));
        }
        let mut progress = latest.unwrap_or_default();
        progress.activity = Some(format!(
            "Invocation telemetry could not be saved: {error}. Work continues; metrics may be incomplete."
        ));
        let _ = request.progress_tx.send(progress);
    }
    match execution {
        Ok(result) => result,
        Err(payload) => std::panic::resume_unwind(payload),
    }
}
