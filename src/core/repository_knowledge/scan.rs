use sha2::Digest;
use std::path::{Path, PathBuf};

use super::{
    RepositoryKnowledgeProfileV1, RepositoryRevisionKeyV1,
    source::{MAX_BLOB_BYTES, command_bytes_with_deadline, valid_oid, valid_path},
};
mod build;
mod cache;
mod compute;
mod git;
use crate::{artifacts::planning_store::PlanningStore, core::project_repos::ProjectManifest};
use build::scan_uncached;
use cache::{cached, save_cache};
use git::{
    checkout_is_dirty, normalize_ref, read_regular_blob as regular_blob, resolve_commit,
    resolve_root,
};

const MAX_FACTS: usize = 128;

/// Request for one explicitly selected repository ref. Callers must bind a
/// task to its source/destination ref before requesting a profile.
pub struct ProfileRequest<'a> {
    pub repository_id: &'a str,
    pub source_ref: &'a str,
    pub code_root: &'a Path,
    pub planning_store: &'a PlanningStore,
    pub repositories: &'a ProjectManifest,
    pub now_unix_ms: u64,
}

/// Owned source coordinates for a profile and subsequent exact-file reads.
#[derive(Clone)]
pub struct ProfileSource {
    pub repository_id: String,
    pub source_ref: String,
    pub code_root: PathBuf,
    pub planning_store: PlanningStore,
    pub repositories: ProjectManifest,
}

impl<'a> From<ProfileRequest<'a>> for ProfileSource {
    fn from(request: ProfileRequest<'a>) -> Self {
        Self {
            repository_id: request.repository_id.to_owned(),
            source_ref: request.source_ref.to_owned(),
            code_root: request.code_root.to_path_buf(),
            planning_store: request.planning_store.clone(),
            repositories: request.repositories.clone(),
        }
    }
}

/// Build or load the bounded local profile for one repository/ref/commit key.
pub fn scan_profile(request: ProfileRequest<'_>) -> Result<RepositoryKnowledgeProfileV1, String> {
    let now = request.now_unix_ms.max(1);
    let source = ProfileSource::from(request);
    let root = resolve_root(&source)?;
    let source_ref = normalize_ref(&source.source_ref)?;
    let commit_sha = resolve_commit(&root, &source_ref)?;
    let dirty_checkout = checkout_is_dirty(&root)?;
    let key = RepositoryRevisionKeyV1 {
        repository_id: source.repository_id.clone(),
        source_ref,
        commit_sha,
        dirty_checkout,
    };
    if let Some(profile) = cached(&source.planning_store, &key) {
        return Ok(profile);
    }
    let profile = scan_uncached(&root, key, now)?;
    save_cache(&source.planning_store, profile.clone());
    Ok(profile)
}

/// Verify that a previously selected profile still names the current commit
/// for its repository ref before a broker accepts any evidence from it.
pub fn verify_profile_freshness(
    source: &ProfileSource,
    profile: &RepositoryKnowledgeProfileV1,
) -> Result<(), String> {
    if source.repository_id != profile.key.repository_id
        || normalize_ref(&source.source_ref)? != profile.key.source_ref
    {
        return Err("profile identity no longer matches the selected repository".into());
    }
    let root = resolve_root(source)?;
    if resolve_commit(&root, &profile.key.source_ref)? != profile.key.commit_sha {
        return Err("profile source revision has changed".into());
    }
    Ok(())
}

/// Cancellable/deadline-bounded profile freshness check for Manager broker calls.
pub fn verify_profile_freshness_with_deadline(
    source: &ProfileSource,
    profile: &RepositoryKnowledgeProfileV1,
    deadline: std::time::Instant,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<(), String> {
    validate_pinned_source_identity(source, &profile.key)?;
    let root = resolve_root_with_deadline(source, deadline, cancelled)?;
    let reference = format!("{}^{{commit}}", profile.key.source_ref);
    let output = command_bytes_with_deadline(
        &root,
        &["rev-parse", "--verify", &reference],
        128,
        deadline,
        cancelled,
    )?;
    let commit = String::from_utf8(output)
        .map_err(|_| "Git returned a non-text commit identity".to_owned())?
        .trim()
        .to_owned();
    if commit != profile.key.commit_sha {
        return Err("profile source revision has changed".into());
    }
    Ok(())
}

/// Re-read one selected file from the exact commit recorded by a profile.
/// The worktree and index are never consulted for file contents.
pub fn read_pinned_file(
    source: &ProfileSource,
    key: &RepositoryRevisionKeyV1,
    relative_path: &str,
) -> Result<(String, String), String> {
    read_pinned_file_bounded(source, key, relative_path, MAX_BLOB_BYTES)
}

/// Read a pinned regular blob only when it fits the caller's remaining byte budget.
pub fn read_pinned_file_bounded(
    source: &ProfileSource,
    key: &RepositoryRevisionKeyV1,
    relative_path: &str,
    max_bytes: usize,
) -> Result<(String, String), String> {
    let never_cancelled = std::sync::atomic::AtomicBool::new(false);
    read_pinned_file_bounded_with_deadline(
        source,
        key,
        relative_path,
        max_bytes,
        std::time::Instant::now() + std::time::Duration::from_secs(30),
        &never_cancelled,
    )
}

/// Pinned regular-blob read with hard output, cancellation, and time bounds.
pub fn read_pinned_file_bounded_with_deadline(
    source: &ProfileSource,
    key: &RepositoryRevisionKeyV1,
    relative_path: &str,
    max_bytes: usize,
    deadline: std::time::Instant,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<(String, String), String> {
    validate_pinned_source_identity(source, key)?;
    if !valid_path(relative_path) {
        return Err("profile file request path is invalid".into());
    }
    let root = resolve_root_with_deadline(source, deadline, cancelled)?;
    let literal_path = format!(":(literal){relative_path}");
    let listing = command_bytes_with_deadline(
        &root,
        &["ls-tree", "-z", &key.commit_sha, "--", &literal_path],
        16 * 1024,
        deadline,
        cancelled,
    )?;
    let object_id = regular_blob(&listing, relative_path)?;
    let size_output = command_bytes_with_deadline(
        &root,
        &["cat-file", "-s", &object_id],
        64,
        deadline,
        cancelled,
    )?;
    let size = String::from_utf8(size_output)
        .map_err(|_| "Git returned an invalid blob size".to_owned())?
        .trim()
        .parse::<usize>()
        .map_err(|_| "Git returned an invalid blob size".to_owned())?;
    if size > MAX_BLOB_BYTES || size > max_bytes {
        return Err("selected repository document exceeds the size limit".into());
    }
    let bytes = command_bytes_with_deadline(
        &root,
        &["cat-file", "blob", &object_id],
        size,
        deadline,
        cancelled,
    )?;
    if bytes.len() != size {
        return Err("selected repository blob changed while being read".into());
    }
    let digest = format!("{:x}", sha2::Sha256::digest(&bytes));
    let content = String::from_utf8(bytes)
        .map_err(|_| "selected repository document is not valid UTF-8".to_owned())?;
    Ok((content, digest))
}

fn validate_pinned_source_identity(
    source: &ProfileSource,
    key: &RepositoryRevisionKeyV1,
) -> Result<(), String> {
    if source.repository_id != key.repository_id
        || source.source_ref != key.source_ref
        || !(key.source_ref.starts_with("refs/heads/")
            || key.source_ref.starts_with("refs/remotes/"))
        || key.source_ref.len() > 256
        || key.source_ref.chars().any(char::is_control)
        || !valid_oid(&key.commit_sha)
    {
        return Err("profile file request does not match its pinned repository identity".into());
    }
    Ok(())
}

fn resolve_root_with_deadline(
    source: &ProfileSource,
    deadline: std::time::Instant,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<PathBuf, String> {
    let root = source
        .repositories
        .target_if_available(
            &source.planning_store,
            &source.code_root,
            &source.repository_id,
        )
        .map_err(|_| "selected code repository is unavailable".to_owned())?
        .ok_or_else(|| "selected code repository has no local checkout".to_owned())?
        .canonicalize()
        .map_err(|_| "selected code repository cannot be opened".to_owned())?;
    let output = command_bytes_with_deadline(
        &root,
        &["rev-parse", "--show-toplevel"],
        4096,
        deadline,
        cancelled,
    )?;
    let top = String::from_utf8(output)
        .map_err(|_| "Git returned a non-text repository root".to_owned())?;
    let top = PathBuf::from(top.trim())
        .canonicalize()
        .map_err(|_| "selected code repository cannot be resolved".to_owned())?;
    if top != root {
        return Err("selected path is not a Git worktree root".into());
    }
    Ok(root)
}
