use super::*;

impl KooladeApp {
    pub(super) fn poll_clone_job(&mut self) {
        // Clone job: drain BEFORE Phase 1's screen borrow so a finished
        // worker can refill `conn_path` and drive submit_connect — the
        // single connect authority. Unfinished workers ride back until
        // they settle (poll rhythm: take → is_finished → put back).
        if let Some(job) = self.clone_job.take() {
            if job.join.is_finished() {
                match job.join.join() {
                    Err(_) => {
                        self.conn_error =
                            Some("The clone worker stopped unexpectedly. Try again.".into());
                    }
                    Ok(Err(e)) => {
                        // Failed clone: `conn_github` is PRESERVED so the
                        // operator can correct and retry; no connect runs.
                        self.conn_error = Some(Self::conn_banner(&e));
                    }
                    Ok(Ok(dest)) => {
                        // Defensive: no navigation exists out of Welcome
                        // while a job runs, so a non-Welcome screen can
                        // only mean something odd — discard silently.
                        if matches!(self.screen, Screen::Welcome) {
                            self.conn_error = None;
                            self.conn_path = dest.to_string_lossy().into_owned();
                            self.submit_connect();
                        }
                    }
                }
            } else {
                self.clone_job = Some(job);
            }
        }
    }
}
