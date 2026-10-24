use crate::{
    execution::{
        contract::{ExecutionModeV1, ResourceBudgetV1, WorkspaceRef},
        docker::images::RuntimeProfile,
        errors::ExecutionErrorKind,
        session::ExecutionSession,
    },
    harness::{AiHarness, HarnessOutcome, PlanningRequest},
};
use std::{
    path::PathBuf,
    sync::{Mutex, mpsc},
    time::Duration,
};

pub(super) struct InitialDispatch {
    pub(super) repository: PathBuf,
    pub(super) workspace: Option<WorkspaceRef>,
    pub(super) has_authorization: bool,
    pub(super) command_succeeded: bool,
    pub(super) detail: String,
}

pub(super) struct RevokedDispatch {
    pub(super) denied_by_policy: bool,
    pub(super) detail: String,
}

pub(super) struct ControllerDockerHarness {
    pub(super) initial: mpsc::Sender<InitialDispatch>,
    pub(super) revoked: mpsc::Sender<RevokedDispatch>,
    pub(super) resume: Mutex<mpsc::Receiver<()>>,
}

impl AiHarness for ControllerDockerHarness {
    fn label(&self) -> String {
        "Evolve controller Docker fixture".into()
    }

    fn check_available(&self) -> Result<String, crate::AppError> {
        Ok(self.label())
    }

    fn execute(&self, request: &PlanningRequest) -> Result<HarnessOutcome, crate::AppError> {
        let repository = request.repo_root.clone();
        let workspace = request.execution_workspace.clone();
        let has_authorization = request.authorization_verifier.is_some();
        let Some(workspace) = workspace.clone() else {
            let _ = self.initial.send(InitialDispatch {
                repository,
                workspace,
                has_authorization,
                command_succeeded: false,
                detail: "controller omitted the execution workspace".into(),
            });
            return Err(fixture_error("controller omitted the execution workspace"));
        };
        let Some(authorization) = request.authorization_verifier.clone() else {
            let _ = self.initial.send(InitialDispatch {
                repository,
                workspace: Some(workspace),
                has_authorization,
                command_succeeded: false,
                detail: "controller omitted the app-owned task authorization".into(),
            });
            return Err(fixture_error(
                "controller omitted the app-owned task authorization",
            ));
        };
        let session = match ExecutionSession::connect_local_with_authorization(
            workspace.clone(),
            &repository,
            ExecutionModeV1::Implementation,
            RuntimeProfile::Rust,
            ResourceBudgetV1::default(),
            Some(authorization),
        ) {
            Ok(session) => session,
            Err(error) => {
                let detail = format!("cannot open authorized Docker session: {error:#}");
                let _ = self.initial.send(InitialDispatch {
                    repository,
                    workspace: Some(workspace),
                    has_authorization,
                    command_succeeded: false,
                    detail: detail.clone(),
                });
                return Err(fixture_error(&detail));
            }
        };
        let command = session.run_tool_command(
            "touch .self-improvement-controller-authorized",
            Duration::from_secs(30),
        );
        let (command_succeeded, detail) = match command {
            Ok(output) => (
                output.result.exit_code == Some(0),
                format!(
                    "exit={:?}; stderr={}",
                    output.result.exit_code, output.stderr
                ),
            ),
            Err(error) => (false, format!("{:?}: {}", error.kind, error.message)),
        };
        if self
            .initial
            .send(InitialDispatch {
                repository,
                workspace: Some(workspace),
                has_authorization,
                command_succeeded,
                detail,
            })
            .is_err()
        {
            return Err(fixture_error(
                "test stopped before observing Docker dispatch",
            ));
        }
        self.resume
            .lock()
            .expect("fixture release receiver is not poisoned")
            .recv_timeout(Duration::from_secs(120))
            .map_err(|error| fixture_error(&format!("cancellation release failed: {error}")))?;

        let cancellation = session.run_tool_command(
            "touch .self-improvement-controller-denied",
            Duration::from_secs(30),
        );
        let revoked = match cancellation {
            Err(error) => RevokedDispatch {
                denied_by_policy: error.kind == ExecutionErrorKind::PolicyDenied,
                detail: error.message,
            },
            Ok(output) => RevokedDispatch {
                denied_by_policy: false,
                detail: format!(
                    "cancelled task unexpectedly ran with exit {:?}",
                    output.result.exit_code
                ),
            },
        };
        let _ = self.revoked.send(revoked);
        Err(fixture_error(
            "controller dispatch fixture ended after cancellation was verified",
        ))
    }
}

fn fixture_error(message: &str) -> crate::AppError {
    crate::AppError::Other(message.into())
}
