use std::sync::mpsc::{Receiver, SyncSender};

use crate::persistence::setup_tasks::SetupTaskStoreV1;

use super::{ApprovalGate, ApprovalPreview, ApprovalRequest, AttemptContext, RemediationPlan};

mod lifecycle;
mod proposal;
mod view;

pub(crate) struct Controller {
    pub(super) gate: Option<ApprovalGate>,
    pub(super) attempt: Option<AttemptContext>,
    pub(super) plan: Option<RemediationPlan>,
    pub(super) preview: Option<ApprovalPreview>,
    pub(super) pending_approval: Option<ApprovalRequest>,
    pub(super) receiver: Option<Receiver<lifecycle::Completion>>,
    pub(super) work: Option<WorkKind>,
    pub(super) result: Option<Result<(), String>>,
    pub(super) store_update: Option<SetupTaskStoreV1>,
    pub(super) authorization_ack: Option<SyncSender<bool>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum WorkKind {
    Preview,
    Execute,
    Cancel,
}

impl Default for Controller {
    fn default() -> Self {
        Self {
            gate: Some(ApprovalGate::default()),
            attempt: None,
            plan: None,
            preview: None,
            pending_approval: None,
            receiver: None,
            work: None,
            result: None,
            store_update: None,
            authorization_ack: None,
        }
    }
}
