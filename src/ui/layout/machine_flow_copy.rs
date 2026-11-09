//! Deterministic, plain-English copy shared by the "Prepare this computer"
//! surfaces (attention-lane parent card and the pre-connect checklist).
//!
//! Copy is derived only from the persisted flow phase and scanned task
//! titles — no provider text ever reaches these widgets.

use crate::persistence::setup_tasks::{DoneOutcomeV1, FlowPhaseV1, HaltReasonV1, SetupFlowStateV1};

/// Which flow-control buttons a given flow position should offer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct FlowControls {
    /// Offer the primary "Fix it for me" button (start, resume, restart).
    pub start: bool,
    /// Offer the pause/stop pair.
    pub pause_and_stop: bool,
}

pub(crate) fn flow_controls(flow: Option<&SetupFlowStateV1>) -> FlowControls {
    match flow {
        None => FlowControls {
            start: true,
            pause_and_stop: false,
        },
        Some(state) => match &state.phase {
            FlowPhaseV1::Running
            | FlowPhaseV1::Finishing
            | FlowPhaseV1::WaitingForConsent { .. }
            | FlowPhaseV1::WaitingForUser { .. } => FlowControls {
                start: false,
                pause_and_stop: true,
            },
            FlowPhaseV1::Paused | FlowPhaseV1::Halted { .. } => FlowControls {
                start: true,
                pause_and_stop: false,
            },
            FlowPhaseV1::Complete(_) => FlowControls {
                start: true,
                pause_and_stop: false,
            },
        },
    }
}

/// One-line status for the card/checklist header.
pub(crate) fn phase_blurb(
    flow: Option<&SetupFlowStateV1>,
    title: impl Fn(&str) -> String,
) -> String {
    let Some(state) = flow else {
        return "Let Kool.ad/e walk this list for you. Anything that touches this computer is explained first and only runs with your approval.".to_owned();
    };
    match &state.phase {
        FlowPhaseV1::Running => "Kool.ad/e is working through this list.".to_owned(),
        FlowPhaseV1::WaitingForConsent { task_id } => {
            format!("One change is waiting for your approval: {}.", title(task_id))
        }
        FlowPhaseV1::WaitingForUser { task_id, reason } => {
            format!("This one needs you: {} ({})", title(task_id), reason_word(*reason))
        }
        FlowPhaseV1::Finishing => "Kool.ad/e is confirming the final state.".to_owned(),
        FlowPhaseV1::Paused => "Paused. Nothing new will be attempted.".to_owned(),
        FlowPhaseV1::Complete(outcome) => match outcome {
            DoneOutcomeV1::FullyPrepared => {
                "Everything checkable on this machine now verifies cleanly.".to_owned()
            }
            DoneOutcomeV1::PreparedWithUnavailable { unavailable_count } => format!(
                "Prepared. {unavailable_count} check(s) cannot run on this operating system and stay marked unavailable."
            ),
        },
        FlowPhaseV1::Halted { reason, .. } => match reason {
            HaltReasonV1::InterruptedByRestart => {
                "Paused after the app restarted. Resume when ready.".to_owned()
            }
            HaltReasonV1::ReadinessChanged => {
                "Paused for review — a check that was ready stopped verifying. Resume after a look-over.".to_owned()
            }
            HaltReasonV1::StoppedByOperator => "Stopped. Resume when ready.".to_owned(),
            HaltReasonV1::RepeatedFailures => {
                "Stopped after repeated trouble. Review the flagged step and retry it manually.".to_owned()
            }
            HaltReasonV1::InternalError => {
                "Stopped to stay safe. Check the setup checklist, then resume.".to_owned()
            }
        },
    }
}

fn reason_word(reason: crate::persistence::setup_tasks::UserActionReasonV1) -> &'static str {
    use crate::persistence::setup_tasks::UserActionReasonV1 as R;
    match reason {
        R::VendorAuthentication => "sign-in step",
        R::OperatingSystemPrompt => "operating-system prompt",
        R::ProductChoice => "your choice to make",
        R::ManualInstructions => "written steps to follow",
        R::DeclinedByOperator => "you declined the offered change",
        R::RepeatedFailure => "several attempts failed",
    }
}
