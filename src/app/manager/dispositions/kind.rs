use crate::{core::planning_work::WorkStatus, harness::prompt_catalog::ManagerActionKind};

#[derive(Clone, Copy)]
pub(super) enum DispositionWork {
    Repair,
    AskUser,
    FollowUp,
    Contest,
}

impl DispositionWork {
    pub(super) fn from_action(action: &ManagerActionKind) -> Option<Self> {
        match action {
            ManagerActionKind::RepairPlan => Some(Self::Repair),
            ManagerActionKind::AskUser => Some(Self::AskUser),
            ManagerActionKind::PromoteFollowUp => Some(Self::FollowUp),
            ManagerActionKind::ContestFinding => Some(Self::Contest),
            _ => None,
        }
    }
    pub(super) fn key(self) -> &'static str {
        match self {
            Self::Repair => "repair",
            Self::AskUser => "ask-user",
            Self::FollowUp => "follow-up",
            Self::Contest => "contest",
        }
    }
    pub(super) fn title(self) -> &'static str {
        match self {
            Self::Repair => "Manager repair",
            Self::AskUser => "Decision needed",
            Self::FollowUp => "Plan follow-up",
            Self::Contest => "Independent arbitration",
        }
    }
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Repair => "RepairPlan",
            Self::AskUser => "AskUser",
            Self::FollowUp => "PromoteFollowUp",
            Self::Contest => "ContestFinding",
        }
    }
    pub(super) fn request(self) -> &'static str {
        match self {
            Self::Repair => "Repair the plan within the confirmed user intent",
            Self::AskUser => "Resolve this product decision with the user",
            Self::FollowUp => "Plan this separate nonblocking follow-up",
            Self::Contest => "Request independent arbitration of this contested finding",
        }
    }
    pub(super) fn status(self) -> WorkStatus {
        match self {
            Self::Repair | Self::FollowUp => WorkStatus::Todo,
            Self::AskUser | Self::Contest => WorkStatus::NeedsAttention,
        }
    }
}
