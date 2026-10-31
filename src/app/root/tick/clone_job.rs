use super::*;

impl KooladeApp {
    pub(super) fn poll_clone_job(&mut self) {
        let Some(job) = self.clone_job.take() else {
            return;
        };
        if !job.join.is_finished() {
            self.clone_job = Some(job);
            return;
        }

        match job.join.join() {
            Err(_) => self.clone_failed(
                "The clone worker stopped unexpectedly. Your repository folders were left in place; retry the setup after checking them.",
            ),
            Ok(Err(error)) => self.clone_failed(&Self::conn_banner(&error)),
            Ok(Ok(destination)) if matches!(self.screen, Screen::Welcome) => {
                self.conn_error = None;
                let path = destination.to_string_lossy().into_owned();
                self.finish_connect(welcome::attempt_connect(&path));
                if matches!(self.screen, Screen::Welcome) {
                    let detail = self.conn_error.clone().unwrap_or_else(|| {
                        "The repositories were paired, but Kool.ad/e could not open the workspace.".into()
                    });
                    self.wiz_error(format!(
                        "The repositories are set up, but the workspace could not open:\n{detail}"
                    ));
                }
            }
            Ok(Ok(_)) => {
                // A navigation path should not leave a completed clone queued
                // for a future welcome session.
                self.wiz = None;
            }
        }
    }

    fn clone_failed(&mut self, detail: &str) {
        self.conn_error = Some(detail.to_owned());
        self.wiz_error(format!(
            "The clone and planning setup did not finish:\n{detail}\nYour code clone and any completed planning work were preserved. Correct the issue and retry."
        ));
    }
}
