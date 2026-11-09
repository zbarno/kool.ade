//! Routing for machine setup repairs.
//!
//! Mirrors the live app dispatch in `app::root::setup_repair` so the
//! deterministic flow driver and the UI agree on which controller owns a
//! given task and when a request may be made.

use crate::persistence::setup_tasks::{SetupTaskV1, State};

/// Controller families a guided repair can target.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FixRoute {
    /// Native/managed remediation broker (rootless Linux, QEMU, WSL features).
    Remediation,
    /// Locked-container image preparation.
    Image,
    /// Claude Code install recipe (separate controller and consent scope).
    ClaudeInstall,
}

pub fn fix_route(task: &SetupTaskV1) -> Option<FixRoute> {
    if is_claude_install_task(&task.id) {
        return Some(FixRoute::ClaudeInstall);
    }
    if is_supported_image_task(&task.id) {
        return Some(FixRoute::Image);
    }
    if is_remediation_task_id(&task.id) {
        return Some(FixRoute::Remediation);
    }
    None
}

/// A fix request may flip the task into its scoped consent review.
pub fn can_request_fix(task: &SetupTaskV1) -> bool {
    fix_route(task).is_some() && task.blocked_by.is_empty() && is_prepare_state(task.state)
}

/// An in-flight review may be approved (or declined) for this task.
pub fn can_decide_fix(task: &SetupTaskV1) -> bool {
    fix_route(task).is_some() && task.blocked_by.is_empty() && task.state == State::NeedsApproval
}

fn is_prepare_state(state: State) -> bool {
    matches!(
        state,
        State::ReadyToFix | State::Failed | State::Paused | State::Interrupted | State::Cancelled
    )
}

fn is_claude_install_task(task_id: &str) -> bool {
    task_id == "setup:cli:claude:install"
}

fn is_supported_image_task(task_id: &str) -> bool {
    crate::app::bootstrap_repair_policy::parse_locked_image_task_id(task_id).is_some()
}

fn is_remediation_task_id(task_id: &str) -> bool {
    matches!(
        task_id,
        "setup:linux:rootless:packages"
            | "setup:linux:rootless:subids"
            | "setup:linux:rootless:service"
            | "setup:computer:qemu"
    ) || windows_setup_task_enabled(task_id)
}

fn windows_setup_task_enabled(task_id: &str) -> bool {
    cfg!(windows)
        && task_id == "setup:windows:wsl-features"
        && crate::windows_setup_helper::publisher_trust_configured()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: &str, state: State) -> SetupTaskV1 {
        SetupTaskV1 {
            schema_version: 1,
            id: id.into(),
            title: String::new(),
            explanation: String::new(),
            next_step: String::new(),
            dependencies: Vec::new(),
            blocked_by: Vec::new(),
            state,
            created_at_unix_ms: 1,
            observed_at_unix_ms: 1,
            scan_generation: 1,
            verified_at_unix_ms: None,
            verified_result: None,
            attempts: Vec::new(),
            events: Vec::new(),
            superseded_by: Vec::new(),
        }
    }

    #[test]
    fn routing_matches_existing_dispatch_table() {
        assert_eq!(
            fix_route(&task("setup:linux:rootless:packages", State::ReadyToFix)),
            Some(FixRoute::Remediation)
        );
        assert_eq!(
            fix_route(&task("setup:computer:qemu", State::ReadyToFix)),
            Some(FixRoute::Remediation)
        );
        assert_eq!(
            fix_route(&task("setup:cli:claude:install", State::ReadyToFix)),
            Some(FixRoute::ClaudeInstall)
        );
        assert_eq!(
            fix_route(&task("setup:docker:engine", State::ReadyToFix)),
            None
        );
        assert_eq!(
            fix_route(&task("setup:cli:codex:install", State::ReadyToFix)),
            None
        );
    }

    #[test]
    fn request_and_decision_gates_track_state_and_prerequisites() {
        let ready = task("setup:computer:qemu", State::ReadyToFix);
        assert!(can_request_fix(&ready));
        assert!(!can_decide_fix(&ready));
        let queued = task("setup:computer:qemu", State::NeedsApproval);
        assert!(!can_request_fix(&queued));
        assert!(can_decide_fix(&queued));
        let blocked = SetupTaskV1 {
            blocked_by: vec!["setup:computer:kvm".into()],
            ..ready
        };
        assert!(!can_request_fix(&blocked));
        assert!(!can_decide_fix(&SetupTaskV1 {
            state: State::NeedsApproval,
            ..blocked
        }));
        for state in [
            State::Detected,
            State::BlockedByPrerequisite,
            State::InProgress,
            State::AwaitingVerification,
            State::NeedsVerification,
            State::Verified,
            State::Unsupported,
        ] {
            assert!(!can_request_fix(&task("setup:computer:qemu", state)));
        }
    }
}
