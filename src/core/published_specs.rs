//! Read published project documents from exact Git objects in the primary
//! code repository. The caller supplies an explicit published ref; the human
//! checkout and task-clone branch are never consulted.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

mod git;
mod navigation;
mod reference;
use git::{
    git, git_bytes, regular_blob_object, valid_object_id, validate_document_path,
    validate_reference,
};
pub use navigation::{NavigationEntry, NavigationIndex};

const MAX_DOCUMENT_BYTES: usize = 1024 * 1024;
const MAX_DOCUMENTS: usize = 256;
const MAX_DOCUMENT_SET_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishedSpecPin {
    pub repository_id: String,
    pub reference: String,
    pub commit_sha: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedDocument {
    pub pin: PublishedSpecPin,
    pub path: String,
    pub content_sha256: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct PublishedSpecReader {
    repository_root: PathBuf,
    repository_id: String,
    published_ref: String,
}

impl PublishedSpecReader {
    pub fn new(
        repository_root: impl Into<PathBuf>,
        repository_id: impl Into<String>,
        published_ref: impl Into<String>,
    ) -> anyhow::Result<Self> {
        let published_ref = published_ref.into();
        validate_reference(&published_ref)?;
        Ok(Self {
            repository_root: repository_root.into(),
            repository_id: repository_id.into(),
            published_ref,
        })
    }

    /// Build a reader for the repository's designated published-spec branch.
    /// The designation is initialized once in private workspace metadata from
    /// a configured remote default, or a conventional/single branch in a
    /// local-only repository. It never treats an arbitrary remote-backed
    /// checkout as published truth. Later checkout changes cannot silently
    /// switch the pinned published ref.
    pub fn for_primary_repository(repository_root: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let repository_root = repository_root.into();
        let reference = reference::designated_ref(&repository_root)?;
        let identity = git(&repository_root, &["remote", "get-url", "origin"])
            .unwrap_or_else(|_| repository_root.to_string_lossy().into_owned());
        let repository_id = format!(
            "primary-code:{:x}",
            Sha256::digest(identity.trim().as_bytes())
        );
        Self::new(repository_root, repository_id, reference)
    }

    /// Resolve a configured full Git ref once and retain its immutable commit.
    pub fn pin(&self) -> anyhow::Result<PublishedSpecPin> {
        let output = git(
            &self.repository_root,
            &[
                "rev-parse",
                "--verify",
                &format!("{}^{{commit}}", self.published_ref),
            ],
        )?;
        let commit_sha = output.trim().to_owned();
        anyhow::ensure!(
            valid_object_id(&commit_sha),
            "Git returned an invalid published-spec commit ID"
        );
        Ok(PublishedSpecPin {
            repository_id: self.repository_id.clone(),
            reference: self.published_ref.clone(),
            commit_sha,
        })
    }

    /// Compare with the existing local ref without fetching or mutating the
    /// code checkout. Callers preparing publication must first refresh their
    /// trusted remote-tracking ref through the normal Git authorization path.
    pub fn ensure_local_ref_unchanged(&self, pin: &PublishedSpecPin) -> anyhow::Result<()> {
        self.validate_pin(pin)?;
        let current = self.pin()?;
        anyhow::ensure!(
            current.commit_sha == pin.commit_sha,
            "published specifications changed from {} to {}; review the new revision before proceeding",
            pin.commit_sha,
            current.commit_sha
        );
        Ok(())
    }

    /// Read a regular Markdown blob by immutable commit and repository path.
    /// The Git index and working tree are not read or modified.
    pub fn read(
        &self,
        pin: &PublishedSpecPin,
        relative_path: &str,
    ) -> anyhow::Result<PublishedDocument> {
        self.read_optional(pin, relative_path)?.ok_or_else(|| {
            anyhow::anyhow!("published document does not exist at the pinned commit")
        })
    }

    /// Read a regular Markdown blob when it exists in the pinned tree. `None`
    /// means the exact path is absent; malformed entries and Git/read failures
    /// remain errors so callers can safely bootstrap only a genuinely missing
    /// documentation index.
    pub fn read_optional(
        &self,
        pin: &PublishedSpecPin,
        relative_path: &str,
    ) -> anyhow::Result<Option<PublishedDocument>> {
        self.validate_pin(pin)?;
        validate_document_path(relative_path)?;
        let listing = git_bytes(
            &self.repository_root,
            &["ls-tree", "-z", &pin.commit_sha, "--", relative_path],
        )?;
        if listing.is_empty() {
            return Ok(None);
        }
        let object_id = regular_blob_object(&listing, relative_path)?;
        let size_output = git(&self.repository_root, &["cat-file", "-s", &object_id])?;
        let size: usize = size_output
            .trim()
            .parse()
            .map_err(|_| anyhow::anyhow!("Git returned an invalid document size"))?;
        anyhow::ensure!(
            size <= MAX_DOCUMENT_BYTES,
            "published document exceeds the {MAX_DOCUMENT_BYTES}-byte limit"
        );
        let bytes = git_bytes(&self.repository_root, &["cat-file", "blob", &object_id])?;
        anyhow::ensure!(
            bytes.len() == size,
            "published document changed while it was being read"
        );
        let content = String::from_utf8(bytes)
            .map_err(|_| anyhow::anyhow!("published document is not valid UTF-8"))?;
        let content_sha256 = format!("{:x}", Sha256::digest(content.as_bytes()));
        Ok(Some(PublishedDocument {
            pin: pin.clone(),
            path: relative_path.to_owned(),
            content_sha256,
            content,
        }))
    }

    /// Read the bounded published product, feature, and architecture documents
    /// from one immutable tree. Navigation indexes remain separate untrusted
    /// hints and are not folded into product truth.
    pub fn read_documentation(
        &self,
        pin: &PublishedSpecPin,
    ) -> anyhow::Result<Vec<PublishedDocument>> {
        self.validate_pin(pin)?;
        let listing = git_bytes(
            &self.repository_root,
            &[
                "ls-tree",
                "-rz",
                "--name-only",
                &pin.commit_sha,
                "--",
                "docs/koolade",
            ],
        )?;
        let paths = listing
            .split(|byte| *byte == 0)
            .filter(|path| !path.is_empty())
            .map(std::str::from_utf8)
            .collect::<Result<Vec<_>, _>>()?;
        anyhow::ensure!(
            paths.len() <= MAX_DOCUMENTS,
            "published documentation contains too many files"
        );
        let mut documents = Vec::new();
        let mut total = 0usize;
        for path in paths {
            let accepted_root = [
                "docs/koolade/product/",
                "docs/koolade/features/",
                "docs/koolade/architecture/",
            ]
            .iter()
            .any(|root| path.starts_with(root));
            if !accepted_root || !path.ends_with(".md") || path.ends_with("/AGENTS.md") {
                continue;
            }
            let document = self.read(pin, path)?;
            total = total.saturating_add(document.content.len());
            anyhow::ensure!(
                total <= MAX_DOCUMENT_SET_BYTES,
                "published documentation exceeds the aggregate size limit"
            );
            documents.push(document);
        }
        Ok(documents)
    }

    fn validate_pin(&self, pin: &PublishedSpecPin) -> anyhow::Result<()> {
        anyhow::ensure!(
            pin.repository_id == self.repository_id,
            "published-spec pin belongs to a different repository"
        );
        validate_reference(&pin.reference)?;
        anyhow::ensure!(
            pin.reference == self.published_ref,
            "published-spec pin does not use the configured published ref"
        );
        anyhow::ensure!(
            valid_object_id(&pin.commit_sha),
            "published-spec pin has an invalid commit ID"
        );
        anyhow::ensure!(
            git(&self.repository_root, &["cat-file", "-t", &pin.commit_sha])?.trim() == "commit",
            "published-spec pin does not identify a commit"
        );
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::duplicate_mod)]
#[path = "published_specs/tests.rs"]
mod tests;
