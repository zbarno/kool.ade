//! Bounded traversal of repository-owned Markdown navigation indexes.
//!
//! Production boundary: navigation is read from exact Git blobs through the
//! caller's commit-pinned reader. A parser-only test cannot establish that
//! candidate documents and their provenance come from the same immutable tree.
//! Tests use synthetic local Git-like blob maps only; no network, SSH, Docker,
//! or credentials are needed, and setup cost is low. Nothing from an index or
//! document is elevated to Manager policy or tool authorization.

use std::collections::{BTreeSet, VecDeque};

use super::{EvidenceKind, EvidenceRefV1, NavigationCandidateV1, ScanOmissionV1};

const MAX_DEPTH: usize = 4;
const MAX_INDEXES: usize = 32;
const MAX_CANDIDATES: usize = 64;
const MAX_INDEX_BYTES: usize = 32 * 1024;
const MAX_TOTAL_BYTES: usize = 256 * 1024;
const MAX_OMISSIONS: usize = 128;
const MAX_PURPOSE_CHARS: usize = 400;
const MAX_SELECTED_DOCUMENT_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, Default)]
pub(super) struct NavigationScan {
    pub candidates: Vec<NavigationCandidateV1>,
    pub omissions: Vec<ScanOmissionV1>,
    pub indexes_read: u16,
    pub documents_read: u16,
    pub bytes_read: u64,
    pub truncated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct SelectedDocument {
    pub content: String,
    pub evidence: EvidenceRefV1,
}

struct IndexRequest {
    path: String,
    depth: usize,
}

mod parse;
use parse::{
    clip_purpose, navigation_entries, omission, resolve_target, sha256, valid_path,
    valid_root_index, valid_sha,
};

/// Follow only the explicit `# Navigation` section and return bounded file
/// candidates. `read` must resolve paths from the exact `source_sha` Git tree.
pub(super) fn scan_navigation(
    root_index: &str,
    source_sha: &str,
    mut read: impl FnMut(&str) -> Result<(String, String), String>,
) -> NavigationScan {
    let mut result = NavigationScan::default();
    let Some(root_dir) = valid_root_index(root_index) else {
        omission(&mut result, root_index, "invalid root navigation index");
        return result;
    };
    if !valid_sha(source_sha) {
        omission(&mut result, root_index, "invalid pinned source SHA");
        return result;
    }

    let mut pending = VecDeque::from([IndexRequest {
        path: root_index.to_owned(),
        depth: 0,
    }]);
    let mut visited = BTreeSet::new();
    let mut candidate_paths = BTreeSet::new();
    let mut total_bytes = 0usize;

    while let Some(request) = pending.pop_front() {
        if visited.contains(&request.path) {
            omission(&mut result, &request.path, "navigation cycle detected");
            continue;
        }
        if request.depth > MAX_DEPTH {
            omission(&mut result, &request.path, "navigation depth limit reached");
            continue;
        }
        if visited.len() >= MAX_INDEXES {
            result.truncated = true;
            omission(
                &mut result,
                &request.path,
                "navigation index count limit reached",
            );
            break;
        }
        visited.insert(request.path.clone());

        let (content, claimed_digest) = match read(&request.path) {
            Ok(value) => value,
            Err(_) => {
                omission(
                    &mut result,
                    &request.path,
                    "navigation index is missing or unreadable at the pinned SHA",
                );
                continue;
            }
        };
        let actual_digest = sha256(content.as_bytes());
        if claimed_digest != actual_digest {
            omission(
                &mut result,
                &request.path,
                "navigation index digest is stale or inconsistent",
            );
            continue;
        }
        if content.len() > MAX_INDEX_BYTES {
            result.truncated = true;
            omission(
                &mut result,
                &request.path,
                "navigation index byte limit reached",
            );
            continue;
        }
        if total_bytes.saturating_add(content.len()) > MAX_TOTAL_BYTES {
            result.truncated = true;
            omission(&mut result, &request.path, "navigation byte budget reached");
            break;
        }
        total_bytes += content.len();
        result.bytes_read = total_bytes as u64;
        result.indexes_read = result.indexes_read.saturating_add(1);

        for entry in navigation_entries(&content) {
            let Some(path) = resolve_target(&request.path, &root_dir, &entry) else {
                omission(
                    &mut result,
                    &request.path,
                    "navigation link is invalid or escapes its documentation root",
                );
                continue;
            };
            if path == "AGENTS.md" || path.ends_with("/AGENTS.md") {
                if visited.contains(&path) || pending.iter().any(|item| item.path == path) {
                    omission(&mut result, &path, "navigation cycle detected");
                } else if request.depth >= MAX_DEPTH {
                    omission(&mut result, &path, "navigation depth limit reached");
                } else {
                    pending.push_back(IndexRequest {
                        path,
                        depth: request.depth + 1,
                    });
                }
                continue;
            }
            if candidate_paths.contains(&path) {
                continue;
            }
            if result.candidates.len() >= MAX_CANDIDATES {
                result.truncated = true;
                omission(&mut result, &path, "navigation candidate limit reached");
                break;
            }
            let (document, document_digest) = match read(&path) {
                Ok(value) => value,
                Err(_) => {
                    omission(
                        &mut result,
                        &path,
                        "navigation target is missing or unreadable at the pinned SHA",
                    );
                    continue;
                }
            };
            if document.len() > MAX_SELECTED_DOCUMENT_BYTES
                || sha256(document.as_bytes()) != document_digest
            {
                omission(
                    &mut result,
                    &path,
                    "navigation target is oversized or has a stale digest",
                );
                continue;
            }
            if total_bytes.saturating_add(document.len()) > MAX_TOTAL_BYTES {
                result.truncated = true;
                omission(
                    &mut result,
                    &path,
                    "navigation byte budget reached before target validation",
                );
                break;
            }
            total_bytes += document.len();
            result.bytes_read = total_bytes as u64;
            result.documents_read = result.documents_read.saturating_add(1);
            candidate_paths.insert(path.clone());
            result.candidates.push(NavigationCandidateV1 {
                path,
                purpose: clip_purpose(&entry.purpose),
                index_path: request.path.clone(),
                index_content_sha256: actual_digest.clone(),
                document_content_sha256: document_digest,
                source_sha: source_sha.to_owned(),
            });
        }
        if result.candidates.len() >= MAX_CANDIDATES {
            result.truncated = true;
            break;
        }
    }
    result
}

/// Read selected document content only when the candidate is an exact member
/// of this profile and binds to its immutable source SHA. Index summaries never
/// substitute for document content.
pub(super) fn read_selected_document(
    profile: &super::RepositoryKnowledgeProfileV1,
    candidate: &NavigationCandidateV1,
    mut read: impl FnMut(&str) -> Result<(String, String), String>,
) -> Result<SelectedDocument, String> {
    if !profile
        .navigation_candidates
        .iter()
        .any(|item| item == candidate)
        || candidate.source_sha != profile.key.commit_sha
        || !valid_sha(&profile.key.commit_sha)
        || !valid_path(&candidate.path)
        || !valid_path(&candidate.index_path)
        || !valid_sha(&candidate.index_content_sha256)
    {
        return Err(
            "selected document is not bound to the profile's pinned repository revision".into(),
        );
    }
    let (content, claimed_digest) = read(&candidate.path)
        .map_err(|_| "selected document is unavailable at the pinned source SHA".to_owned())?;
    if content.len() > MAX_SELECTED_DOCUMENT_BYTES {
        return Err("selected document exceeds its byte limit".into());
    }
    let digest = sha256(content.as_bytes());
    if claimed_digest != digest {
        return Err("selected document digest does not match its pinned content".into());
    }
    if digest != candidate.document_content_sha256 {
        return Err("selected document changed since navigation indexing".into());
    }
    let line_end = u32::try_from(content.lines().count()).unwrap_or(u32::MAX);
    Ok(SelectedDocument {
        content,
        evidence: EvidenceRefV1 {
            path: candidate.path.clone(),
            symbol: None,
            line_start: (line_end > 0).then_some(1),
            line_end: (line_end > 0).then_some(line_end),
            content_sha256: digest,
            source_sha: candidate.source_sha.clone(),
            kind: EvidenceKind::Documentation,
        },
    })
}

#[cfg(test)]
#[path = "navigation/tests.rs"]
mod tests;
