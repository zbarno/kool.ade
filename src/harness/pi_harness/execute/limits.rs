use std::{fs::File, path::PathBuf, sync::mpsc::Sender, time::Duration};

use crate::{
    error::AppError,
    harness::{LiveProgress, pi_proc::ChildTask},
};

use super::super::tail;

pub(super) fn fail(
    task: &ChildTask,
    diagnostics: Option<&(PathBuf, File)>,
    reason: String,
    progress_tx: &Sender<LiveProgress>,
    stderr_tail: &[String],
) -> AppError {
    task.kill();
    let _ = task.settle(Duration::from_secs(3));
    let diagnostic_path = diagnostics
        .map(|(path, _)| path.display().to_string())
        .unwrap_or_else(|| "not recorded".into());
    let reason = format!(
        "{reason}; execution stopped before the tool ran; diagnostic events: {diagnostic_path}"
    );
    let _ = progress_tx.send(LiveProgress {
        activity: Some(reason.clone()),
        ..Default::default()
    });
    AppError::HarnessFailed {
        reason,
        stderr_tail: tail(stderr_tail),
    }
}
