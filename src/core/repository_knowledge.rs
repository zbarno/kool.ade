//! Revision-pinned, local-only architecture profiles for code repositories.
//!
//! Repository files, including navigation indexes, are retrieved as evidence
//! and never promoted to Manager instructions or authority.

mod facts;
mod navigation;
mod profile;
mod scan;
mod source;

pub use profile::{
    ArchitectureClaimV1, ClaimEvidenceV1, ClaimStatus, EvidenceKind, EvidenceRefV1,
    KnowledgeProfileStatus, NavigationCandidateV1, RepositoryKnowledgeProfileV1,
    RepositoryRevisionKeyV1, ScanOmissionV1, ScanStatsV1,
};
pub use scan::{
    ProfileRequest, ProfileSource, read_pinned_file, read_pinned_file_bounded,
    read_pinned_file_bounded_with_deadline, scan_profile, scan_profile_with_cache,
    verify_profile_freshness, verify_profile_freshness_with_deadline,
};

/// Re-read and validate a selected navigation candidate at its pinned commit.
pub fn read_selected_document(
    source: &ProfileSource,
    profile: &RepositoryKnowledgeProfileV1,
    candidate: &NavigationCandidateV1,
) -> Result<(String, EvidenceRefV1), String> {
    navigation::read_selected_document(profile, candidate, |path| {
        read_pinned_file(source, &profile.key, path)
    })
    .map(|selected| (selected.content, selected.evidence))
}

#[cfg(test)]
#[path = "repository_knowledge/tests.rs"]
mod tests;
