use super::*;
use crate::core::workflow::{self, TurnPurpose};
use crate::ui::feature_approval::Action;

#[path = "feature_approval/actions.rs"]
mod actions;
#[path = "feature_approval/approval.rs"]
mod approval;
#[path = "feature_approval/available.rs"]
mod available;
#[path = "feature_approval/board.rs"]
mod board;
#[path = "feature_approval/continuation.rs"]
mod continuation;
#[path = "feature_approval/readiness.rs"]
pub(super) mod readiness;
#[path = "feature_approval/request_changes.rs"]
mod request_changes;
#[path = "feature_approval/retry_review.rs"]
mod retry_review;
#[path = "feature_approval/selection.rs"]
mod selection;

#[cfg(test)]
#[path = "root/feature_approval/tests.rs"]
pub(crate) mod tests;
