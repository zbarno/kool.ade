use crate::app::{
    bootstrap_approval::ApprovalPreview,
    bootstrap_repair_policy::{Host, Operation, Proposal},
};

pub(super) fn preview(prerequisite_id: &str) -> Result<(Proposal, ApprovalPreview), String> {
    let proposal = proposal(prerequisite_id)?;
    let gate = crate::app::bootstrap_approval::ApprovalGate::new();
    let runtime = runtime()?;
    let preview = runtime
        .block_on(gate.preview(&proposal))
        .map_err(|error| error.to_string())?;
    Ok((proposal, preview))
}

pub(super) fn proposal(prerequisite_id: &str) -> Result<Proposal, String> {
    let architecture = prerequisite_id
        .strip_prefix("setup:images:general:")
        .filter(|value| matches!(*value, "amd64" | "arm64"))
        .ok_or_else(|| "Only a canonical general runtime image can be prepared.".to_owned())?;
    Ok(Proposal {
        schema_version: 1,
        operation: Operation::PrepareLockedImage,
        host: Host::Linux,
        prerequisite_id: format!("setup:images:general:{architecture}"),
        image_profile: Some("general".into()),
    })
}

pub(super) fn prepare_approved(
    approval: crate::app::bootstrap_approval::ApprovedAction,
    proposal: &Proposal,
) -> Result<(), String> {
    if approval.operation() != Operation::PrepareLockedImage
        || approval.prerequisite_id() != proposal.prerequisite_id
        || approval.image_profile() != Some("general")
        || proposal.image_profile.as_deref() != Some("general")
    {
        return Err("The one-use approval does not authorize this general image.".into());
    }
    approval
        .prepare_image(proposal)
        .map_err(|error| error.to_string())?;
    Ok(())
}

pub(super) fn runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("Could not start the image preparation worker: {error}"))
}
