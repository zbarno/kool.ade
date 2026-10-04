use super::*;

pub(super) fn cleanup_runner() -> Runner {
    let (progress, _rx) = mpsc::channel();
    Runner {
        gh: "unused".into(),
        deadline: Instant::now() + Duration::from_secs(30),
        cancel: Arc::new(AtomicBool::new(false)),
        progress,
    }
}

// Produce a completed record without invoking automatic cleanup: models an
// older Koolade version leaving a merged PR's worktree behind.
