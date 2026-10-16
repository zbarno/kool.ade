use crate::harness::prompt_catalog::SpecDeltaV1;
use sha2::Digest as _;

pub(super) fn delta_summary(proposal: &SpecDeltaV1) -> String {
    let details = proposal
        .deltas
        .iter()
        .take(16)
        .map(|delta| {
            format!(
                "{}: {} → {} ({:?}, approval={:?})",
                delta.requirement_id,
                crate::core::context_build::clip(&delta.old, 200),
                crate::core::context_build::clip(&delta.new, 200),
                delta.classification,
                delta.approval_effect
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    format!(
        "SpecReconciler proposed {} requirement delta(s): {details}",
        proposal.deltas.len()
    )
}

pub(super) fn hash_json(value: &impl serde::Serialize) -> Option<String> {
    serde_json::to_vec(value)
        .ok()
        .map(|value| format!("{:x}", sha2::Sha256::digest(value)))
}
