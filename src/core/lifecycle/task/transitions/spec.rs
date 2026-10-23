use crate::core::lifecycle::{
    GateActor, GateDecision, GateReceipt, QaExecutionStatusV1, TaskRecord, types::is_sha256,
};

impl TaskRecord {
    pub(super) fn valid_spec_receipt(&self, receipt: &GateReceipt) -> bool {
        self.implementation_sha256.as_deref() == Some(receipt.subject_sha256.as_str())
            && receipt.subject_uid == self.task_uid
            && receipt.actor == GateActor::ApplicationPolicy
            && receipt.decision == GateDecision::Approved
            && receipt.validate().is_ok()
    }

    pub(super) fn valid_spec_blocker(
        &self,
        reason: &str,
        proposal_sha256: &str,
        spec_sha256: &str,
    ) -> bool {
        !reason.trim().is_empty()
            && reason.len() <= 4096
            && is_sha256(proposal_sha256)
            && is_sha256(spec_sha256)
            && self
                .qa_execution
                .as_ref()
                .is_some_and(|state| state.status == QaExecutionStatusV1::Completed)
            && self.runtime_qa.as_ref().is_some_and(|receipt| {
                receipt.decision == GateDecision::Approved
                    && Some(receipt.subject_sha256.as_str())
                        == self.implementation_sha256.as_deref()
            })
    }
}
