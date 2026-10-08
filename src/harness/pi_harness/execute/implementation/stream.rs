use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use crate::error::AppError;
use crate::harness::pi_events::EventFold;
use crate::harness::pi_proc::StreamEvt;
use crate::harness::resource_bridge::ResourceBridge;
use crate::harness::{HarnessOutcome, PlanningRequest};

use super::super::super::read_budget::PlanningReadBudget;
use super::super::super::{compress_completed_events, configured_stall_timeout, tail};
use super::super::limits;

pub(super) fn run(
    task: crate::harness::pi_proc::ChildTask,
    req: &PlanningRequest,
    mut diagnostics: Option<(PathBuf, std::fs::File)>,
    resource_bridge: Option<&ResourceBridge>,
    planning_reads: bool,
) -> Result<HarnessOutcome, AppError> {
    let deadline = Instant::now() + req.timeout;
    let stall_limit = configured_stall_timeout();
    let stall_secs = stall_limit.as_secs();
    let mut last_output = Instant::now();
    let mut fold = EventFold::default();
    let mut stderr_tail: Vec<String> = Vec::new();
    let mut last_preview = crate::harness::LiveProgress::default();
    let mut last_emit = Instant::now() - Duration::from_millis(50);
    let mut preview_dirty = false;
    let mut planning_read_budget = PlanningReadBudget::default();

    loop {
        if req.cancel.load(std::sync::atomic::Ordering::Relaxed) {
            task.kill();
            let _ = task.settle(Duration::from_secs(3));
            return Err(AppError::HarnessFailed {
                reason: "cancelled by user".into(),
                stderr_tail: tail(&stderr_tail),
            });
        }
        if Instant::now() >= deadline {
            task.kill();
            let _ = task.settle(Duration::from_secs(3));
            return Err(AppError::HarnessTimedOut {
                secs: req.timeout.as_secs(),
            });
        }
        if preview_dirty && last_emit.elapsed() >= Duration::from_millis(50) {
            let preview = fold.preview();
            if preview != last_preview {
                let _ = req.progress_tx.send(preview.clone());
                last_preview = preview;
            }
            preview_dirty = false;
            last_emit = Instant::now();
        }
        // NOTE: `Pending` is NOT an error state — the first event from a
        // cold harness can lag well past one poll window.
        match task.poll_next(Duration::from_millis(200)) {
            Err(crate::harness::pi_proc::PollState::Pending) => {
                if last_output.elapsed() >= stall_limit {
                    task.kill();
                    let _ = task.settle(Duration::from_secs(3));
                    return Err(AppError::HarnessFailed {
                        reason: format!(
                            "harness stalled: pi produced no output for {stall_secs}s and was presumed hung"
                        ),
                        stderr_tail: tail(&stderr_tail),
                    });
                }
                continue;
            }
            Ok(StreamEvt::Stdout(line)) => {
                last_output = Instant::now();
                if let Some((_, file)) = diagnostics.as_mut() {
                    use std::io::Write;
                    writeln!(file, "{line}").map_err(|e| {
                        AppError::Other(format!("Cannot write harness diagnostics: {e}"))
                    })?;
                }
                if planning_reads && let Err(reason) = planning_read_budget.observe_line(&line) {
                    return Err(limits::fail(
                        &task,
                        diagnostics.as_ref(),
                        reason,
                        &req.progress_tx,
                        &stderr_tail,
                    ));
                }
                crate::harness::pi_events::fold_line(&line, &mut fold);
                let usage_preview = fold.preview();
                if usage_preview.model_calls != last_preview.model_calls {
                    let _ = req.progress_tx.send(usage_preview.clone());
                    last_preview = usage_preview;
                    preview_dirty = false;
                    last_emit = Instant::now();
                } else {
                    preview_dirty = true;
                }
            }
            Ok(StreamEvt::Stderr(line)) => {
                last_output = Instant::now();
                stderr_tail.push(line);
                if stderr_tail.len() > 100 {
                    stderr_tail.remove(0);
                }
            }
            Ok(StreamEvt::Exited(ok)) => {
                if let Some(error) = resource_attention(resource_bridge, &req.progress_tx) {
                    return Err(error);
                }
                if !ok {
                    return Err(AppError::HarnessFailed {
                        reason: fold
                            .error_hint
                            .clone()
                            .unwrap_or_else(|| "pi exited with a failure code".into()),
                        stderr_tail: tail(&stderr_tail),
                    });
                }
                break;
            }
            Err(crate::harness::pi_proc::PollState::Closed) => {
                if let Some(error) = resource_attention(resource_bridge, &req.progress_tx) {
                    return Err(error);
                }
                // Pipe disconnected without an Exited event (defensive).
                if !fold.saw_agent_end && fold.final_assistant_text.is_empty() {
                    return Err(AppError::HarnessFailed {
                        reason: "stream ended unexpectedly".into(),
                        stderr_tail: tail(&stderr_tail),
                    });
                }
                break;
            }
        }
    }

    let _ = req.progress_tx.send(fold.preview());
    if let Some(error) = resource_attention(resource_bridge, &req.progress_tx) {
        return Err(error);
    }
    let final_text = std::mem::take(&mut fold.final_assistant_text);
    if final_text.trim().is_empty() {
        return Err(AppError::HarnessFailed {
            reason: format!(
                "pi finished but produced no final assistant message ({} parsed events, {} unparsed lines, agent_end={}, last_stop_reason={}, last_error_class={}, tool_executions={}; diagnostics: {})",
                fold.events_seen,
                fold.unparsed_lines,
                fold.saw_agent_end,
                fold.last_stop_reason.as_deref().unwrap_or("unknown"),
                fold.last_error_class.unwrap_or("unknown"),
                fold.tool_executions,
                diagnostics
                    .as_ref()
                    .map(|(path, _)| path.display().to_string())
                    .unwrap_or_else(|| "not recorded (no writable Git metadata)".into())
            ),
            stderr_tail: tail(&stderr_tail),
        });
    }
    // Keep a raw stream through the run so a crash or failed invocation
    // leaves a readable diagnostic at the path named in its error. Once a
    // final answer exists, archive that stream losslessly. Compression is
    // best effort: the raw file remains if gzip is unavailable or fails.
    if let Some((path, file)) = diagnostics.take() {
        drop(file);
        let _ = compress_completed_events(&path);
    }
    Ok(HarnessOutcome {
        final_text,
        envelope: None,
        stderr_tail: tail(&stderr_tail),
    })
}

fn resource_attention(
    bridge: Option<&crate::harness::resource_bridge::ResourceBridge>,
    progress: &std::sync::mpsc::Sender<crate::harness::LiveProgress>,
) -> Option<AppError> {
    let bridge = bridge?;
    if let Some(request) = bridge.dependency_request() {
        let detail = request.summary();
        let _ = progress.send(crate::harness::LiveProgress {
            activity: Some("Dependency request needs authorization".into()),
            dependency_requests: vec![request.clone()],
            ..Default::default()
        });
        if request.status != crate::harness::DependencyRequestStatus::Prepared {
            return Some(AppError::Other(format!(
                "Needs Attention: a specific dependency request needs review. {detail}"
            )));
        }
    }
    bridge.attention_detail().map(|detail| AppError::Other(format!(
        "Needs Attention: the worker requested a resource that requires operator review. {detail}"
    )))
}
