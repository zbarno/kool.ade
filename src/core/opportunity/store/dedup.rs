use crate::{core::opportunity::OpportunityV1, harness::prompt_catalog::OpportunityProposalV1};

pub(super) fn has_materially_new_evidence(
    existing: &OpportunityV1,
    existing_prompt: Option<&OpportunityProposalV1>,
    candidate: &OpportunityV1,
    candidate_prompt: Option<&OpportunityProposalV1>,
) -> bool {
    match (existing_prompt, candidate_prompt) {
        (Some(existing_prompt), Some(candidate_prompt)) => {
            let existing_hashes = existing_prompt
                .evidence_refs
                .iter()
                .zip(&existing.evidence_refs)
                .map(|(id, evidence)| (id.as_str(), evidence.sha256.as_str()))
                .filter(|(id, _)| *id != "workspace-summary")
                .collect::<std::collections::HashSet<_>>();
            candidate_prompt
                .evidence_refs
                .iter()
                .zip(&candidate.evidence_refs)
                .any(|(id, evidence)| {
                    id != "workspace-summary"
                        && !existing_hashes.contains(&(id.as_str(), evidence.sha256.as_str()))
                })
        }
        _ => {
            let existing_hashes = existing
                .evidence_refs
                .iter()
                .map(|evidence| evidence.sha256.as_str())
                .collect::<std::collections::HashSet<_>>();
            candidate
                .evidence_refs
                .iter()
                .any(|evidence| !existing_hashes.contains(evidence.sha256.as_str()))
        }
    }
}
