use super::{ClaudeInstallController, ClaudeInstallProgress, ClaudeInstallStatus};

impl ClaudeInstallController {
    pub(crate) fn status(&self) -> &ClaudeInstallStatus {
        &self.status
    }

    pub(crate) fn progress(&self) -> Option<ClaudeInstallProgress> {
        self.progress
    }
}
