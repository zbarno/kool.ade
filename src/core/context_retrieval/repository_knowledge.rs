use sha2::Digest;
use std::{collections::BTreeMap, path::PathBuf, process::Command, sync::Arc};

use crate::core::{
    repository_knowledge::{
        NavigationCandidateV1, ProfileRequest, ProfileSource, RepositoryKnowledgeProfileV1,
        read_selected_document,
    },
    state::PlannerState,
};

const MAX_PROFILES: usize = 8;
const MAX_CANDIDATES: usize = 48;

pub(super) struct RepositoryEvidence {
    pub summary: String,
    pub candidates: BTreeMap<String, RepositoryDocument>,
    pub areas: BTreeMap<String, super::RetrievedArea>,
    pub cache_hits: usize,
}

pub(super) struct RepositoryDocument {
    pub source: ProfileSource,
    pub profile: Arc<RepositoryKnowledgeProfileV1>,
    pub candidate: NavigationCandidateV1,
}

pub(super) fn collect(state: &PlannerState) -> RepositoryEvidence {
    let mut summary = String::new();
    let mut candidates = BTreeMap::new();
    let mut areas = BTreeMap::new();
    let mut cache_hits = 0;
    for repository in state.repositories.repositories.iter().take(MAX_PROFILES) {
        let Ok(Some(root)) = state.repositories.target_if_available(
            &state.planning_store,
            &state.repo_root,
            &repository.id,
        ) else {
            summary.push_str(&format!(
                "Repository {}: local checkout unavailable; architecture is unknown.\n",
                repository.id
            ));
            continue;
        };
        let Some(source_ref) = branch_ref(&root) else {
            summary.push_str(&format!(
                "Repository {}: branch ref unavailable; architecture is unknown.\n",
                repository.id
            ));
            continue;
        };
        let request = ProfileRequest {
            repository_id: &repository.id,
            source_ref: &source_ref,
            code_root: &state.repo_root,
            planning_store: &state.planning_store,
            repositories: &state.repositories,
            now_unix_ms: now_unix_ms(),
        };
        let Ok((profile, cached)) =
            crate::core::repository_knowledge::scan_profile_with_cache(request)
        else {
            summary.push_str(&format!(
                "Repository {} at {}: profile unavailable; architecture is unknown.\n",
                repository.id, source_ref
            ));
            continue;
        };
        cache_hits += usize::from(cached);
        let status = format!("{:?}", profile.status).to_ascii_lowercase();
        summary.push_str(&format!(
            "Repository {} at checked-out ref {} commit {} status {} dirty {} languages {:?}. This evidence describes the labeled ref only and does not establish that a task branch was merged into another ref.\n",
            profile.key.repository_id,
            profile.key.source_ref,
            profile.key.commit_sha,
            status,
            profile.key.dirty_checkout,
            profile.languages
        ));
        let mut profile_summary = String::new();
        for claim in profile
            .entry_points_and_interfaces
            .iter()
            .chain(&profile.top_level_subsystems)
            .chain(&profile.build_and_test_conventions)
            .chain(&profile.dependencies)
            .chain(&profile.execution_boundaries)
            .chain(&profile.observed_capabilities)
            .take(64)
        {
            profile_summary.push_str(&format!(
                "- {}: {} [{}]",
                claim.area,
                claim.statement,
                format!("{:?}", claim.evidence.status).to_ascii_lowercase()
            ));
            for reference in claim.evidence.references.iter().take(3) {
                profile_summary.push_str(&format!(
                    " {}:{}-{}@{}",
                    reference.path,
                    reference.line_start.unwrap_or(1),
                    reference
                        .line_end
                        .unwrap_or(reference.line_start.unwrap_or(1)),
                    reference.source_sha
                ));
            }
            profile_summary.push('\n');
        }
        summary.push_str(&profile_summary);
        let area_id = format!(
            "repo-profile:{}:{}:{}",
            profile.key.repository_id,
            profile.key.source_ref,
            &profile.key.commit_sha[..12]
        );
        let content_sha256 = format!("{:x}", sha2::Sha256::digest(profile_summary.as_bytes()));
        areas.insert(
            area_id,
            super::RetrievedArea {
                repository_id: profile.key.repository_id.clone(),
                source_ref: profile.key.source_ref.clone(),
                source_revision: profile.key.commit_sha.clone(),
                path: "profile://architecture".into(),
                content_sha256,
                content: profile_summary,
            },
        );
        let profile = Arc::new(profile);
        for candidate in profile.navigation_candidates.iter().take(MAX_CANDIDATES) {
            let id = format!(
                "repo-doc:{}:{}:{}:{}",
                profile.key.repository_id,
                profile.key.source_ref,
                &profile.key.commit_sha[..12],
                candidate.path
            );
            candidates.insert(
                id,
                RepositoryDocument {
                    source: ProfileSource {
                        repository_id: repository.id.clone(),
                        source_ref: source_ref.clone(),
                        code_root: state.repo_root.clone(),
                        planning_store: state.planning_store.clone(),
                        repositories: state.repositories.clone(),
                    },
                    profile: Arc::clone(&profile),
                    candidate: candidate.clone(),
                },
            );
        }
    }
    RepositoryEvidence {
        summary,
        candidates,
        areas,
        cache_hits,
    }
}

pub(super) fn read_selected(document: &RepositoryDocument) -> Option<(String, String)> {
    let (content, evidence) =
        read_selected_document(&document.source, &document.profile, &document.candidate).ok()?;
    Some((content, evidence.content_sha256))
}

fn branch_ref(root: &PathBuf) -> Option<String> {
    let output = Command::new("git")
        .args(["symbolic-ref", "--quiet", "--short", "HEAD"])
        .current_dir(root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    (!branch.is_empty()).then(|| format!("refs/heads/{branch}"))
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(1)
}
