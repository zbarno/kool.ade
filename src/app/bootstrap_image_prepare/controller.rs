use std::sync::mpsc::{Receiver, SyncSender};

use crate::app::bootstrap_approval::{ApprovalGate, ApprovalPreview, ApprovalRequest};
use crate::app::bootstrap_repair_policy::Proposal;
use crate::persistence::setup_tasks::SetupTaskStoreV1;

mod lifecycle;
mod view;

use lifecycle::{Completion, RecoveryAction};

#[derive(Clone)]
pub(super) struct AttemptContext {
    pub(super) task_id: String,
    pub(super) operation_id: String,
    pub(super) version: u16,
    pub(super) requested_scopes: Vec<String>,
}

#[derive(Clone, Copy)]
pub(super) enum WorkKind {
    Preview,
    Prepare,
    Cancel,
}

pub(crate) struct Controller {
    gate: Option<ApprovalGate>,
    proposal: Option<Proposal>,
    preview: Option<ApprovalPreview>,
    pending_approval: Option<ApprovalRequest>,
    attempt: Option<AttemptContext>,
    receiver: Option<Receiver<Completion>>,
    work_kind: Option<WorkKind>,
    result: Option<Result<(), String>>,
    store_update: Option<SetupTaskStoreV1>,
    authorization_publication_ack: Option<SyncSender<bool>>,
    recovery_action: Option<RecoveryAction>,
}

impl Default for Controller {
    fn default() -> Self {
        Self {
            gate: Some(ApprovalGate::new()),
            proposal: None,
            preview: None,
            pending_approval: None,
            attempt: None,
            receiver: None,
            work_kind: None,
            result: None,
            store_update: None,
            authorization_publication_ack: None,
            recovery_action: None,
        }
    }
}
