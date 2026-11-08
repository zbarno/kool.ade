use super::{InvestigationApprovedRevisionV1, InvestigationRequestV1, InvestigationRevisionV1};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// App-owned immutable authorization material staged for one Manager call.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InvestigationAuthorizationV1 {
    pub code_root: PathBuf,
    pub planning_root: PathBuf,
    pub receipt_root: PathBuf,
    pub project_id: String,
    pub repository_id: String,
    pub source_ref: String,
    pub source_sha: String,
    pub workspace_id: String,
    pub task_id: String,
    pub cancellation_id: String,
    pub task_revision: String,
    pub profile: crate::core::repository_knowledge::RepositoryKnowledgeProfileV1,
    pub repositories: crate::core::project_repos::ProjectManifest,
    #[serde(default)]
    pub approved_comparison_revisions: Vec<InvestigationApprovedRevisionV1>,
}

impl InvestigationAuthorizationV1 {
    pub fn context(&self) -> Result<super::super::InvestigationContext, String> {
        self.profile.validate()?;
        if self.approved_comparison_revisions.len() > 8
            || self.approved_comparison_revisions.iter().any(|pin| {
                pin.intent.validate().is_err()
                    || pin.intent.repository_id != self.repository_id
                    || normalize_source_ref(&pin.intent.source_ref).is_err()
            })
        {
            return Err("approved comparison revision authorization is invalid".into());
        }
        if self.profile.key.repository_id != self.repository_id
            || self.profile.key.source_ref != self.source_ref
            || self.profile.key.commit_sha != self.source_sha
            || self.workspace_id
                != format!("{}:{}:{}", self.project_id, self.task_id, self.source_sha)
        {
            return Err("investigation authorization material is inconsistent".into());
        }
        let project_id = uuid::Uuid::parse_str(&self.project_id)
            .map_err(|_| "investigation project identity is invalid")?;
        let planning_store = crate::artifacts::planning_store::PlanningStore::for_code_repository(
            project_id,
            self.planning_root.clone(),
            self.code_root.clone(),
        );
        Ok(super::super::InvestigationContext {
            source: crate::core::repository_knowledge::ProfileSource {
                repository_id: self.repository_id.clone(),
                source_ref: self.source_ref.clone(),
                code_root: self.code_root.clone(),
                planning_store,
                repositories: self.repositories.clone(),
            },
            profile: self.profile.clone(),
            workspace_id: self.workspace_id.clone(),
            task_id: self.task_id.clone(),
            cancellation_id: self.cancellation_id.clone(),
            task_revision: self.task_revision.clone(),
            comparison: None,
        })
    }

    pub fn context_for_request(
        &self,
        request: &InvestigationRequestV1,
    ) -> Result<super::super::InvestigationContext, String> {
        let mut context = self.context()?;
        let Some(revision) = &request.comparison else {
            return Ok(context);
        };
        let approved = self.approved_comparison_revisions.iter().any(|pin| {
            pin.intent.repository_id == self.repository_id
                && normalize_source_ref(&pin.intent.source_ref).ok().as_deref()
                    == Some(revision.source_ref.as_str())
                && pin.intent.source_sha == revision.source_sha
        });
        if !approved {
            return Err("comparison revision is not app-approved for this repository".into());
        }
        let profile = crate::core::repository_knowledge::scan_profile(
            crate::core::repository_knowledge::ProfileRequest {
                repository_id: &self.repository_id,
                source_ref: &revision.source_ref,
                code_root: &self.code_root,
                planning_store: &context.source.planning_store,
                repositories: &self.repositories,
                now_unix_ms: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|_| "system clock is unavailable")?
                    .as_millis()
                    .min(u64::MAX as u128) as u64,
            },
        )
        .unwrap_or_else(|_| unavailable_profile(&self.repository_id, revision));
        context.comparison = Some(super::super::InvestigationRevisionContext {
            source: crate::core::repository_knowledge::ProfileSource {
                repository_id: self.repository_id.clone(),
                source_ref: revision.source_ref.clone(),
                code_root: self.code_root.clone(),
                planning_store: context.source.planning_store.clone(),
                repositories: self.repositories.clone(),
            },
            profile,
        });
        Ok(context)
    }
}

fn normalize_source_ref(source_ref: &str) -> Result<String, String> {
    let reference = if source_ref.starts_with("refs/heads/") {
        source_ref.to_owned()
    } else {
        format!("refs/heads/{source_ref}")
    };
    crate::core::git_intent::validate_ref(&reference, "comparison")
        .map_err(|_| "approved comparison branch ref is invalid".to_owned())?;
    Ok(reference)
}

fn unavailable_profile(
    repository_id: &str,
    revision: &InvestigationRevisionV1,
) -> crate::core::repository_knowledge::RepositoryKnowledgeProfileV1 {
    use crate::core::repository_knowledge::{
        KnowledgeProfileStatus, RepositoryKnowledgeProfileV1, RepositoryRevisionKeyV1, ScanStatsV1,
    };
    RepositoryKnowledgeProfileV1 {
        schema_version: 1,
        key: RepositoryRevisionKeyV1 {
            repository_id: repository_id.to_owned(),
            source_ref: revision.source_ref.clone(),
            commit_sha: revision.source_sha.clone(),
            dirty_checkout: false,
        },
        scanned_at_unix_ms: 1,
        status: KnowledgeProfileStatus::Unavailable,
        languages: Vec::new(),
        manifests: Vec::new(),
        build_and_test_conventions: Vec::new(),
        top_level_subsystems: Vec::new(),
        entry_points_and_interfaces: Vec::new(),
        dependencies: Vec::new(),
        execution_boundaries: Vec::new(),
        observed_capabilities: Vec::new(),
        navigation_candidates: Vec::new(),
        omissions: Vec::new(),
        scan: ScanStatsV1::default(),
    }
}
