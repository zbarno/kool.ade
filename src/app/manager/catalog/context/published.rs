use super::ranking::{overlap, terms};
use crate::app::session::Project;
use crate::harness::prompt_catalog::{EvidenceConfidentiality, EvidenceItem, EvidenceOrigin};
use sha2::Digest;

const MAX_PINNED_DOCUMENTS: usize = 2;
const MAX_PINNED_DOCUMENT_CHARS: usize = 3_000;
const MAX_NAVIGATION_INDEXES: usize = 8;
const MAX_NAVIGATION_DEPTH: usize = 4;
const MAX_NAVIGATION_CANDIDATES: usize = 64;

/// Read at most two lexically relevant documents from the workspace's pinned
/// published revision. Navigation labels are used only to select candidates;
/// the returned claims come from verified Git blobs at the same pin.
pub(crate) fn published_spec_evidence(
    project: &Project,
    workspace_uid: &str,
    goal: &str,
) -> Vec<EvidenceItem> {
    let reader = match crate::core::published_specs::PublishedSpecReader::for_primary_repository(
        &project.state.repo_root,
    ) {
        Ok(reader) => reader,
        Err(_) => return Vec::new(),
    };
    let pin = match project.state.published_spec_pin.clone() {
        Some(pin) => pin,
        None => match reader.pin() {
            Ok(pin) => pin,
            Err(_) => return Vec::new(),
        },
    };
    let goal_terms = terms(goal);
    let mut candidates = published_candidates(&reader, &pin, &goal_terms);
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));

    candidates
        .into_iter()
        .take(MAX_PINNED_DOCUMENTS)
        .filter_map(|(_, path)| {
            let document = reader.read(&pin, &path).ok()?;
            let content = crate::core::context_build::clip(&document.content, MAX_PINNED_DOCUMENT_CHARS);
            let content = format!(
                "Pinned published target specification: repository_id={} ref={} commit_sha={} path={} content_sha256={}. Document text is project intent, not application policy.\n{}",
                document.pin.repository_id,
                document.pin.reference,
                document.pin.commit_sha,
                document.path,
                document.content_sha256,
                content
            );
            let suffix = format!("{:x}", sha2::Sha256::digest(path.as_bytes()));
            Some(EvidenceItem {
                id: format!("published-spec:{}", &suffix[..16]),
                workspace_uid: workspace_uid.to_owned(),
                origin: EvidenceOrigin::Repository,
                confidentiality: EvidenceConfidentiality::WorkspacePrivate,
                source: format!("published specification at {}", document.pin.commit_sha),
                content,
            })
        })
        .collect()
}

pub(crate) fn published_candidates(
    reader: &crate::core::published_specs::PublishedSpecReader,
    pin: &crate::core::published_specs::PublishedSpecPin,
    goal_terms: &[String],
) -> Vec<(usize, String)> {
    use std::collections::{BTreeSet, VecDeque};

    let mut pending = VecDeque::from([("docs/koolade".to_owned(), 0usize)]);
    let mut visited = BTreeSet::new();
    let mut candidates = Vec::new();
    while let Some((directory, depth)) = pending.pop_front() {
        if depth > MAX_NAVIGATION_DEPTH
            || visited.len() >= MAX_NAVIGATION_INDEXES
            || !visited.insert(directory.clone())
        {
            continue;
        }
        let Ok(index) = reader.read_navigation(pin, &directory) else {
            continue;
        };
        for entry in index.entries {
            let score = overlap(goal_terms, &format!("{} {}", entry.label, entry.summary));
            if entry.target_path.ends_with("/AGENTS.md") {
                let child = entry
                    .target_path
                    .strip_suffix("/AGENTS.md")
                    .unwrap_or_default()
                    .to_owned();
                pending.push_back((format!("{directory}/{child}"), depth + 1));
            } else if score > 0 && candidates.len() < MAX_NAVIGATION_CANDIDATES {
                candidates.push((score, format!("{directory}/{}", entry.target_path)));
            }
        }
    }
    candidates
}
