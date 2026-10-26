use super::{ApprovalGate, ApprovalRequest, ApprovedAction};
use crate::app::bootstrap_repair_policy::Proposal;

/// Simulate the confirmation button in a fixture while retaining the
/// production preview, one-use ticket, audit, and consume checks. This is
/// compiled only into tests; shipped builds still require the real UI request.
pub(crate) async fn approve_image_for_test(
    proposal: &Proposal,
) -> anyhow::Result<(String, ApprovedAction)> {
    let mut gate = ApprovalGate::new();
    let preview = gate.preview(proposal).await?;
    let request = ApprovalRequest {
        proposal: preview.proposal.clone(),
        binding: preview.binding.clone(),
    };
    let ticket = gate.grant(request)?;
    let receipt_id = ticket.to_string();
    let approved = gate.consume(ticket, proposal).await?;
    Ok((receipt_id, approved))
}
