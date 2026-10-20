use super::super::{KooladeApp, Screen};

impl KooladeApp {
    pub(super) fn approve_code_review_risk_command(
        &mut self,
        work_id: String,
        task_uid: String,
        finding_id: String,
        subject_hash: String,
    ) {
        match &mut self.screen {
            Screen::Connected(project) => match crate::app::manager::approve_code_review_risk(
                project,
                &work_id,
                &task_uid,
                &finding_id,
                &subject_hash,
            ) {
                Ok(()) => self
                    .toasts
                    .success("Residual code-review risk accepted for this finding."),
                Err(error) => self.toasts.warning(error),
            },
            Screen::Welcome => self
                .toasts
                .warning("Connect a workspace before accepting code-review risk."),
        }
    }
}
