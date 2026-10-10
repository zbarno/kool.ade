use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use super::CandidateDocument;
use crate::artifacts::planning_store::PlanningRoot;
use crate::core::context_build::clip;
use crate::domain::ArtifactIdentity;

const MAX_PRODUCT_CANDIDATES: usize = 32;
const MAX_CHANGE_CANDIDATES: usize = 64;
const MAX_CANDIDATE_BYTES: usize = 64 * 1024;
const MAX_EXCERPT_BYTES: usize = 4 * 1024;

pub(super) fn product_documents<R: PlanningRoot + ?Sized>(
    repo: &R,
) -> BTreeMap<String, CandidateDocument> {
    let mut result = BTreeMap::new();
    let Ok(Some(manifest)) = crate::artifacts::product_docs::load_manifest(repo) else {
        return result;
    };
    let root = repo.planning_layout().product_root();
    for module in manifest.modules.iter().take(MAX_PRODUCT_CANDIDATES) {
        let Some(path) = crate::artifacts::product_docs::safe_module_path(&root, &module.path)
        else {
            continue;
        };
        let Some(body) = read_bounded(repo, &path, MAX_EXCERPT_BYTES) else {
            continue;
        };
        if crate::artifacts::product_docs::validate_module(&body).is_err()
            || crate::artifacts::product_docs::module_title(&body) != Some(module.title.as_str())
        {
            continue;
        }
        insert_document(
            &mut result,
            repo,
            format!("product:{}", module.id),
            &path,
            &body,
        );
    }
    result
}

pub(super) fn add_recent_changes<R: PlanningRoot + ?Sized>(
    repo: &R,
    documents: &mut BTreeMap<String, CandidateDocument>,
) {
    let layout = repo.planning_layout();
    let root = layout.changes_root();
    if !real_directory(&root) {
        return;
    }
    let Ok(entries) = fs::read_dir(&root) else {
        return;
    };
    let mut entries = entries.flatten().collect::<Vec<_>>();
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.file_name()));
    let mut added = documents
        .keys()
        .filter(|id| id.starts_with("change:"))
        .count();
    let mut scanned = 0;
    for entry in entries {
        scanned += 1;
        if added >= MAX_CHANGE_CANDIDATES || scanned > MAX_CHANGE_CANDIDATES * 3 {
            break;
        }
        let Ok(meta) = fs::symlink_metadata(entry.path()) else {
            continue;
        };
        if !meta.is_dir() || meta.file_type().is_symlink() {
            continue;
        }
        let Some(path) = layout.change_specification(&entry.file_name().to_string_lossy()) else {
            continue;
        };
        if !regular_file(&path) {
            continue;
        }
        let Some(body) = read_bounded(repo, &path, MAX_EXCERPT_BYTES) else {
            continue;
        };
        let id = ArtifactIdentity::from_markdown(&body)
            .ok()
            .flatten()
            .map(|identity| identity.display_id)
            .or_else(|| {
                body.lines().find_map(|line| {
                    line.strip_prefix("# ")?
                        .split_once(':')
                        .map(|(id, _)| id.trim().to_owned())
                })
            })
            .unwrap_or_else(|| entry.file_name().to_string_lossy().into_owned());
        let key = format!("change:{id}");
        if !documents.contains_key(&key) {
            insert_document(documents, repo, key, &path, &body);
            added += 1;
        }
    }
}

pub(super) fn find_change_path<R: PlanningRoot + ?Sized>(
    repo: &R,
    id: &str,
    body: &str,
) -> Option<PathBuf> {
    let layout = repo.planning_layout();
    let root = layout.changes_root();
    if !real_directory(&root) {
        return None;
    }
    let entries = fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let directory = layout.change_directory(&name.to_string_lossy())?;
        if !real_directory(&directory) {
            continue;
        }
        let Some(path) = layout.change_specification(&name.to_string_lossy()) else {
            continue;
        };
        if !regular_file(&path) {
            continue;
        }
        let Some(candidate) = read_bounded(repo, &path, MAX_EXCERPT_BYTES) else {
            continue;
        };
        let identity = ArtifactIdentity::from_markdown(&candidate).ok().flatten();
        let header_id = candidate.lines().find_map(|line| {
            line.strip_prefix("# ")?
                .split_once(':')
                .map(|(id, _)| id.trim())
        });
        if identity
            .as_ref()
            .is_some_and(|identity| identity.display_id == id)
            || header_id == Some(id)
            || candidate == body
        {
            return Some(path);
        }
    }
    None
}

pub(super) fn insert_document<R: PlanningRoot + ?Sized>(
    documents: &mut BTreeMap<String, CandidateDocument>,
    repo: &R,
    id: String,
    path: &Path,
    body: &str,
) {
    let root = repo.planning_layout().root().to_path_buf();
    let Some(source_path) = relative(&root, path) else {
        return;
    };
    if !regular_file(path) {
        return;
    }
    let Some(canonical) = safe_candidate_path(&root, path) else {
        return;
    };
    let content =
        read_bounded(repo, &canonical, MAX_EXCERPT_BYTES).unwrap_or_else(|| body.to_owned());
    let excerpt = excerpt(&content);
    documents.entry(id.clone()).or_insert(CandidateDocument {
        id,
        source_path,
        excerpt,
    });
}

pub(super) fn load_selected<R: PlanningRoot + ?Sized>(
    repo: &R,
    candidate: &CandidateDocument,
    cap: usize,
) -> Option<super::RetrievedDocument> {
    let root = repo.planning_layout().root().to_path_buf();
    let path = safe_source_path(&root, &candidate.source_path)?;
    let content = read_bounded(repo, &path, MAX_CANDIDATE_BYTES)?;
    Some(super::RetrievedDocument {
        id: candidate.id.clone(),
        source_path: candidate.source_path.clone(),
        content: clip(&content, cap),
    })
}

fn excerpt(body: &str) -> String {
    let mut lines = body
        .lines()
        .filter(|line| !line.starts_with("<!-- koolade-artifact-id:"));
    let heading = lines
        .next()
        .unwrap_or_default()
        .trim_start_matches('#')
        .trim();
    let summary = lines
        .filter(|line| {
            let line = line.trim();
            !line.is_empty() && !line.starts_with('#') && !line.starts_with("**Status:**")
        })
        .take(3)
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(" ");
    clip(&format!("{heading}: {summary}"), 420).replace('\n', " ")
}

fn real_directory(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
}

fn regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
}

fn relative(repo: &Path, path: &Path) -> Option<String> {
    let path = path.strip_prefix(repo).ok()?;
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return None;
    }
    Some(path.to_string_lossy().replace('\\', "/"))
}

fn read_bounded<R: PlanningRoot + ?Sized>(
    repo: &R,
    path: &Path,
    max_bytes: usize,
) -> Option<String> {
    let mut bytes = repo.read_planning_path(path).ok()?;
    bytes.truncate(max_bytes);
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

fn safe_candidate_path(repo: &Path, path: &Path) -> Option<PathBuf> {
    let relative = path.strip_prefix(repo).ok()?;
    safe_relative_path(repo, relative)
}

fn safe_source_path(repo: &Path, source_path: &str) -> Option<PathBuf> {
    safe_relative_path(repo, Path::new(source_path))
}

fn safe_relative_path(repo: &Path, relative: &Path) -> Option<PathBuf> {
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return None;
    }
    let canonical_repo = repo.canonicalize().ok()?;
    let candidate = canonical_repo.join(relative);
    let components = relative.components().collect::<Vec<_>>();
    let mut current = canonical_repo.clone();
    for (index, component) in components.iter().enumerate() {
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current).ok()?;
        if metadata.file_type().is_symlink()
            || (index + 1 == components.len() && !metadata.is_file())
            || (index + 1 < components.len() && !metadata.is_dir())
        {
            return None;
        }
    }
    let canonical = candidate.canonicalize().ok()?;
    canonical.starts_with(&canonical_repo).then_some(candidate)
}
