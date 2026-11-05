//! Prepare an explicitly approved documentation change as a protected GitHub PR.
//!
//! The publisher uses a private isolated Git repository and index. It never checks out
//! or writes to the operator's code working tree, and pushes only a fresh
//! `koolade/spec-*` branch.

mod git;
mod github;
mod initial;
mod publish;
mod validation;

use crate::core::published_specs::{PublishedSpecPin, PublishedSpecReader};
#[cfg(test)]
use std::collections::BTreeSet;

#[cfg(test)]
use git::ensure_existing_path_is_safe;
#[cfg(test)]
use github::github_repository_from_remote;
pub use initial::initial_product_documents;
pub(crate) use publish::publish_approved;
#[cfg(test)]
use publish::publish_with_gh;
use validation::digest_changes;
pub(crate) use validation::validate_changes;

const MAX_DOCUMENT_BYTES: usize = 1024 * 1024;
const MAX_DOCUMENT_COUNT: usize = 64;
const MAX_TOTAL_DOCUMENT_BYTES: usize = 4 * 1024 * 1024;
const MAX_OUTPUT_BYTES: usize = 16 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentationChange {
    pub path: String,
    pub content: String,
}

/// Confirmation is bound to the exact document set presented for publication.
/// The UI creates this only in response to the operator's explicit publish action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PublicationConsent {
    approved_document_digest: String,
}

impl PublicationConsent {
    pub(crate) fn confirmed(changes: &[DocumentationChange]) -> anyhow::Result<Self> {
        validate_changes(changes)?;
        Ok(Self {
            approved_document_digest: digest_changes(changes),
        })
    }

    pub(crate) fn digest(&self) -> &str {
        &self.approved_document_digest
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullRequest {
    pub url: String,
    pub branch: String,
    pub base_commit: String,
    pub head_commit: String,
}

#[cfg(test)]
pub(crate) fn publish_with_test_gh(
    repository_root: &std::path::Path,
    repository_id: &str,
    pin: &PublishedSpecPin,
    changes: &[DocumentationChange],
    consent: &PublicationConsent,
    gh: &std::ffi::OsStr,
) -> anyhow::Result<PullRequest> {
    publish_with_gh(repository_root, repository_id, pin, changes, consent, gh)
}

#[cfg(test)]
#[path = "spec_publication/tests.rs"]
mod tests;
