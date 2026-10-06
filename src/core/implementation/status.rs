//! Typed, versionable implementation and recovery state.
mod failure;

pub use failure::{Failure, FailureCause, FailureKind, RecoveryDisposition};

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImplementationStatus {
    Preparing,
    Implementing,
    Verifying,
    ReadyToPublish,
    AwaitingApproval,
    ChangesRequested,
    Publishing,
    WaitingForIndependentChecks,
    AwaitingReview,
    WaitingToMerge,
    Blocked,
    Completed,
    PullRequestClosed,
    Interrupted,
}

impl ImplementationStatus {
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::Preparing => "preparing",
            Self::Implementing => "implementing",
            Self::Verifying => "verifying",
            Self::ReadyToPublish => "ready_to_publish",
            Self::AwaitingApproval => "awaiting_approval",
            Self::ChangesRequested => "changes_requested",
            Self::Publishing => "publishing",
            Self::WaitingForIndependentChecks => "waiting_for_independent_checks",
            Self::AwaitingReview => "awaiting_review",
            Self::WaitingToMerge => "waiting_to_merge",
            Self::Blocked => "blocked",
            Self::Completed => "completed",
            Self::PullRequestClosed => "pull_request_closed",
            Self::Interrupted => "interrupted",
        }
    }

    pub fn parse_wire(value: &str) -> Option<Self> {
        Some(match value {
            "preparing" => Self::Preparing,
            "implementing" => Self::Implementing,
            "verifying" => Self::Verifying,
            "ready_to_publish" => Self::ReadyToPublish,
            "awaiting_approval" => Self::AwaitingApproval,
            "changes_requested" => Self::ChangesRequested,
            "publishing" => Self::Publishing,
            "waiting_for_independent_checks" => Self::WaitingForIndependentChecks,
            "awaiting_review" => Self::AwaitingReview,
            "waiting_to_merge" => Self::WaitingToMerge,
            "blocked" => Self::Blocked,
            "completed" => Self::Completed,
            "pull_request_closed" => Self::PullRequestClosed,
            "interrupted" => Self::Interrupted,
            _ => return None,
        })
    }

    /// One-time v0-state migration. New persistence uses `wire_name`.
    pub fn parse_legacy_label(value: &str) -> Option<Self> {
        Self::parse_wire(value).or_else(|| {
            Some(match value {
                "Preparing" => Self::Preparing,
                "Implementing" => Self::Implementing,
                "Verifying" => Self::Verifying,
                "Ready for PR" => Self::ReadyToPublish,
                "Approval required" => Self::AwaitingApproval,
                "Changes requested" => Self::ChangesRequested,
                "Publishing" => Self::Publishing,
                "Waiting for independent checks" => Self::WaitingForIndependentChecks,
                "PR created" => Self::AwaitingReview,
                "Waiting to merge" => Self::WaitingToMerge,
                "Needs attention" => Self::Blocked,
                "Done" => Self::Completed,
                "PR closed" => Self::PullRequestClosed,
                "Interrupted" => Self::Interrupted,
                _ => return None,
            })
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Preparing => "Preparing",
            Self::Implementing => "Implementing",
            Self::Verifying => "Verifying",
            Self::ReadyToPublish => "Ready to share",
            Self::AwaitingApproval => "Approval required",
            Self::ChangesRequested => "Changes requested",
            Self::Publishing => "Publishing",
            Self::WaitingForIndependentChecks => "Waiting for project checks",
            Self::AwaitingReview => "PR created",
            Self::WaitingToMerge => "Waiting to merge",
            Self::Blocked => "Needs attention",
            Self::Completed => "Done",
            Self::PullRequestClosed => "PR closed",
            Self::Interrupted => "Interrupted",
        }
    }

    pub fn publication(self) -> PublicationStatus {
        match self {
            Self::ReadyToPublish | Self::AwaitingApproval | Self::ChangesRequested => {
                PublicationStatus::Ready
            }
            Self::Publishing | Self::WaitingForIndependentChecks => PublicationStatus::Publishing,
            Self::AwaitingReview | Self::WaitingToMerge => PublicationStatus::Published,
            Self::PullRequestClosed => PublicationStatus::Closed,
            Self::Completed => PublicationStatus::Complete,
            _ => PublicationStatus::NotStarted,
        }
    }
}

impl fmt::Display for ImplementationStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublicationStatus {
    NotStarted,
    Ready,
    Publishing,
    Published,
    Closed,
    Complete,
}

/// Result returned by a configured independent CI provider for the exact
/// candidate commit that Koolade locally verified.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndependentCheckStatus {
    Pending,
    Passed,
    Failed,
    Unavailable,
}

impl IndependentCheckStatus {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Pending => "Waiting",
            Self::Passed => "Passed",
            Self::Failed => "Failed",
            Self::Unavailable => "Unavailable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndependentCheck {
    pub provider: String,
    pub commit: String,
    pub candidate_ref: String,
    pub status: IndependentCheckStatus,
    pub checked_at: Option<String>,
    pub detail: Option<String>,
}

impl PublicationStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::NotStarted => "Not started",
            Self::Ready => "Ready to publish",
            Self::Publishing => "Publishing",
            Self::Published => "Published",
            Self::Closed => "Closed",
            Self::Complete => "Complete",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PullRequestState {
    Open,
    Closed,
    Merged,
}

impl PullRequestState {
    pub fn parse_api(value: &str) -> Option<Self> {
        Some(match value {
            "OPEN" => Self::Open,
            "CLOSED" => Self::Closed,
            "MERGED" => Self::Merged,
            _ => return None,
        })
    }

    pub fn parse_wire(value: &str) -> Option<Self> {
        Some(match value {
            "open" => Self::Open,
            "closed" => Self::Closed,
            "merged" => Self::Merged,
            _ => return None,
        })
    }

    pub fn parse_legacy(value: &str) -> Option<Self> {
        Self::parse_wire(value).or(match value {
            "OPEN" => Some(Self::Open),
            "CLOSED" => Some(Self::Closed),
            "MERGED" => Some(Self::Merged),
            _ => None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Open => "OPEN",
            Self::Closed => "CLOSED",
            Self::Merged => "MERGED",
        }
    }
}
