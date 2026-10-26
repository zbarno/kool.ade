use crate::app::{
    bootstrap_approval::{ApprovalPreview, ImagePreparationReceipt},
    bootstrap_repair_policy::{Host, Operation, Proposal, parse_locked_image_task_id},
};
use crate::execution::docker::images::RuntimeProfile;

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
    let (profile, _, _) = parse_locked_image_task_id(prerequisite_id)
        .ok_or_else(|| "Only a canonical approved runtime image can be prepared.".to_owned())?;
    Ok(Proposal {
        schema_version: 1,
        operation: Operation::PrepareLockedImage,
        host: Host::Linux,
        prerequisite_id: prerequisite_id.to_owned(),
        image_profile: Some(profile.into()),
    })
}

pub(super) fn prepare_approved(
    approval: crate::app::bootstrap_approval::ApprovedAction,
    proposal: &Proposal,
    pull_control: &crate::execution::docker::engine::ImagePullControl,
) -> Result<ImagePreparationReceipt, String> {
    let (profile, architecture, engine_identity) =
        parse_locked_image_task_id(&proposal.prerequisite_id)
            .ok_or_else(|| "The approved image task identity is invalid.".to_owned())?;
    if approval.operation() != Operation::PrepareLockedImage
        || approval.prerequisite_id() != proposal.prerequisite_id
        || approval.image_profile() != Some(profile)
        || proposal.image_profile.as_deref() != Some(profile)
    {
        return Err("The one-use approval does not authorize this runtime image.".into());
    }
    let receipt = approval
        .prepare_image(proposal, pull_control)
        .map_err(|error| error.to_string())?;
    let profile = runtime_profile(profile)
        .ok_or_else(|| "The approved runtime profile is invalid.".to_owned())?;
    if receipt.profile != profile
        || receipt.architecture != architecture
        || receipt.engine_identity_fingerprint != engine_identity
    {
        return Err("The image smoke receipt does not match the approved task identity.".into());
    }
    let expected = crate::execution::docker::images::ImageLock::bundled()
        .and_then(|lock| lock.select(profile, architecture))
        .map_err(|error| error.message)?;
    if receipt.image_digest != expected.digest {
        return Err("The image smoke receipt does not match the bundled image digest.".into());
    }
    Ok(receipt)
}

fn runtime_profile(profile: &str) -> Option<RuntimeProfile> {
    Some(match profile {
        "general" => RuntimeProfile::General,
        "rust" => RuntimeProfile::Rust,
        "node" => RuntimeProfile::Node,
        "python" => RuntimeProfile::Python,
        "go" => RuntimeProfile::Go,
        _ => return None,
    })
}

pub(super) fn runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("Could not start the image preparation worker: {error}"))
}
