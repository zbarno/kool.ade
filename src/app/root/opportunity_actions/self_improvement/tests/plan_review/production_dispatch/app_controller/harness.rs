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
    pub(super) command_failure: Option<CommandFailure>,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum CommandFailure {
    Session(crate::execution::errors::ExecutionErrorKind),
    Command(crate::execution::errors::ExecutionErrorKind, &'static str),
    Exit(Option<i32>),
}

impl CommandFailure {
    pub(super) fn description(&self) -> String {
        match self {
            Self::Session(kind) => format!("session:{kind:?}"),
            Self::Command(kind, category) => format!("command:{kind:?}/{category}"),
            Self::Exit(code) => format!("exit:{code:?}"),
        }
    }

    pub(super) fn command(error: &crate::execution::errors::ExecutionError) -> Self {
        Self::Command(error.kind, safe_command_failure_category(error))
    }
}

fn safe_command_failure_category(error: &crate::execution::errors::ExecutionError) -> &'static str {
    let message = error.message.to_ascii_lowercase();
    if message.contains("another execution already owns this task clone") {
        "task-lock"
    } else if message.contains("self-improvement")
        || message.contains("authorization")
        || message.contains("task claim")
    {
        "authorization-fence"
    } else if message.contains("workspace") || message.contains("task clone") {
        "workspace-fence"
    } else if message.contains("request") || message.contains("policy") {
        "request-policy"
    } else if message.contains("sandbox") || message.contains("image") {
        "sandbox-policy"
    } else if error.kind == crate::execution::errors::ExecutionErrorKind::PolicyDenied {
        "other-policy-denial"
    } else {
        "runtime-failure"
    }
}

pub(super) struct RevokedDispatch {
    pub(super) denied_by_policy: bool,
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
                command_failure: None,
            });
            return Err(fixture_error("controller omitted the execution workspace"));
        };
        let Some(authorization) = request.authorization_verifier.clone() else {
            let _ = self.initial.send(InitialDispatch {
                repository,
                workspace: Some(workspace),
                has_authorization,
                command_succeeded: false,
                command_failure: None,
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
                let _ = self.initial.send(InitialDispatch {
                    repository,
                    workspace: Some(workspace),
                    has_authorization,
                    command_succeeded: false,
                    command_failure: Some(CommandFailure::Session(error.kind)),
                });
                return Err(fixture_error("cannot open authorized Docker session"));
            }
        };
        let (command_succeeded, command_failure) = match session.run_tool_command(
            "touch .self-improvement-controller-authorized",
            Duration::from_secs(30),
        ) {
            Ok(output) if output.result.exit_code == Some(0) => (true, None),
            Ok(output) => (false, Some(CommandFailure::Exit(output.result.exit_code))),
            Err(error) => (false, Some(CommandFailure::command(&error))),
        };
        if self
            .initial
            .send(InitialDispatch {
                repository,
                workspace: Some(workspace),
                has_authorization,
                command_succeeded,
                command_failure,
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
        let denied_by_policy = matches!(
            cancellation,
            Err(error) if error.kind == ExecutionErrorKind::PolicyDenied
        );
        let revoked = RevokedDispatch { denied_by_policy };
        let _ = self.revoked.send(revoked);
        Err(fixture_error(
            "controller dispatch fixture ended after cancellation was verified",
        ))
    }
}

fn fixture_error(message: &str) -> crate::AppError {
    crate::AppError::Other(message.into())
}
