//! Deterministic management classification for machine setup tasks.
//!
//! Given a persisted task, decide who can move it forward: the scoped
//! consent-gated repair pipeline, the operator personally, nobody (the OS
//! lacks the capability), or the operator's judgment after exhausted
//! automatic retries. Unknown IDs always resolve to a human step: the flow
//! never infers an autonomy grant.

use crate::persistence::setup_tasks::{SetupTaskV1, State, UserActionReasonV1};

use super::routes;

/// Automatic retry allowance before a repair hands back to the operator.
pub const RETRY_LIMIT: usize = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Management {
    /// A guided repair exists; its own consent gate controls execution.
    Repairable { queued: bool },
    /// Only the operator reasonably completes this step.
    OperatorStep { reason: UserActionReasonV1 },
    /// Automatic retries exhausted; operator judgment is required.
    Exhausted,
    /// This machine cannot run the check at all.
    Unavailable,
}

impl Management {
    pub fn is_repairable(self) -> bool {
        matches!(self, Self::Repairable { .. })
    }
    pub fn is_operator_barrier(self) -> bool {
        matches!(self, Self::OperatorStep { .. } | Self::Exhausted)
    }
    pub fn operator_reason(self) -> Option<UserActionReasonV1> {
        match self {
            Self::OperatorStep { reason } => Some(reason),
            Self::Exhausted => Some(UserActionReasonV1::RepeatedFailure),
            _ => None,
        }
    }
}

/// Call for non-complete tasks (neither Verified nor Superseded).
pub fn classify(task: &SetupTaskV1) -> Management {
    if task.state == State::Unsupported {
        return Management::Unavailable;
    }
    if routes::fix_route(task).is_some() {
        return Management::Repairable {
            queued: task.state == State::NeedsApproval,
        };
    }
    Management::OperatorStep {
        reason: operator_reason(&task.id),
    }
}

pub fn failed_attempt_count(task: &SetupTaskV1) -> usize {
    task.attempts
        .iter()
        .filter(|attempt| attempt.state == crate::persistence::setup_tasks::AttemptState::Failed)
        .count()
}

fn operator_reason(task_id: &str) -> UserActionReasonV1 {
    if task_id.starts_with("setup:cli:") {
        return cli_reason(task_id);
    }
    match task_id {
        "setup:computer:kvm"
        | "setup:windows:virtualization"
        | "setup:windows:wsl-policy"
        | "setup:windows:wsl-kernel"
        | "setup:windows:reboot"
        | "setup:windows:host-version"
        | "setup:docker:permission"
        | "setup:docker:disk"
        | "setup:docker:limits" => UserActionReasonV1::OperatingSystemPrompt,
        "setup:docker:architecture"
        | "setup:docker:image-architecture"
        | "setup:docker:windows-containers"
        | "setup:docker:image-digest"
        | "setup:docker:image-unverified" => UserActionReasonV1::ProductChoice,
        _ => UserActionReasonV1::ManualInstructions,
    }
}

fn cli_reason(task_id: &str) -> UserActionReasonV1 {
    if matches!(task_id, "setup:cli:install" | "setup:cli:select") {
        return UserActionReasonV1::ProductChoice;
    }
    let Some(rest) = task_id.strip_prefix("setup:cli:") else {
        return UserActionReasonV1::ManualInstructions;
    };
    let mut segments = rest.split(':');
    let Some(stage) = segments.nth(1) else {
        return UserActionReasonV1::ManualInstructions;
    };
    match stage {
        "signin" | "auth" | "authenticate" | "auth-verify" => {
            UserActionReasonV1::VendorAuthentication
        }
        "models" | "experimental" => UserActionReasonV1::ProductChoice,
        _ => UserActionReasonV1::ManualInstructions,
    }
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
    fn repair_routed_tasks_are_managed_through_their_consent_gates() {
        for id in [
            "setup:linux:rootless:packages",
            "setup:linux:rootless:subids",
            "setup:linux:rootless:service",
            "setup:computer:qemu",
            "setup:cli:claude:install",
        ] {
            assert_eq!(
                classify(&task(id, State::ReadyToFix)),
                Management::Repairable { queued: false }
            );
            assert_eq!(
                classify(&task(id, State::NeedsApproval)),
                Management::Repairable { queued: true }
            );
        }
    }

    #[test]
    fn unrouted_tasks_escalate_to_the_operator_never_autonomy() {
        let cases = [
            (
                "setup:docker:engine",
                UserActionReasonV1::ManualInstructions,
            ),
            (
                "setup:docker:permission",
                UserActionReasonV1::OperatingSystemPrompt,
            ),
            (
                "setup:computer:kvm",
                UserActionReasonV1::OperatingSystemPrompt,
            ),
            ("setup:cli:install", UserActionReasonV1::ProductChoice),
            (
                "setup:cli:codex:auth",
                UserActionReasonV1::VendorAuthentication,
            ),
            ("setup:cli:codex:models", UserActionReasonV1::ProductChoice),
            (
                "setup:cli:totally-new:weird-stage",
                UserActionReasonV1::ManualInstructions,
            ),
            (
                "setup:zzz:future:id",
                UserActionReasonV1::ManualInstructions,
            ),
        ];
        for (id, reason) in cases {
            let management = classify(&task(id, State::Detected));
            assert_eq!(management.operator_reason(), Some(reason), "{id}");
            assert!(!management.is_repairable(), "{id}");
        }
    }

    #[test]
    fn unsupported_machines_report_unavailable() {
        assert_eq!(
            classify(&task("setup:docker:engine", State::Unsupported)),
            Management::Unavailable
        );
    }
}
