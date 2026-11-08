use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const MAX_QUESTION_CHARS: usize = 2_000;
const MAX_SCOPE_PATHS: usize = 32;
const MAX_PATH_CHARS: usize = 512;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InvestigationBudgetV1 {
    pub max_files: u16,
    pub max_bytes: u32,
    pub max_matches: u16,
    pub max_elapsed_ms: u32,
}

impl InvestigationBudgetV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.max_files == 0
            || self.max_files > 32
            || self.max_bytes == 0
            || self.max_bytes > 262_144
            || self.max_matches == 0
            || self.max_matches > 128
            || self.max_elapsed_ms == 0
            || self.max_elapsed_ms > 10_000
        {
            return Err("investigation budget is outside the permitted bounds".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InvestigationScopeV1 {
    pub paths: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InvestigationRequestV1 {
    pub idempotency_key: String,
    pub repository_id: String,
    pub source_ref: String,
    pub source_sha: String,
    pub workspace_id: String,
    pub task_id: String,
    pub expected_task_revision: String,
    pub question: String,
    pub allowed_evidence_classes: Vec<crate::core::repository_knowledge::EvidenceKind>,
    pub scope: InvestigationScopeV1,
    pub budget: InvestigationBudgetV1,
    pub cancellation_id: String,
    pub query: String,
}

impl InvestigationRequestV1 {
    pub fn validate(&self) -> Result<(), String> {
        for (id, label) in [
            (&self.idempotency_key, "idempotency key"),
            (&self.task_id, "task ID"),
            (&self.cancellation_id, "cancellation ID"),
        ] {
            let parsed = uuid::Uuid::parse_str(id).map_err(|_| format!("invalid {label}"))?;
            if parsed.hyphenated().to_string() != *id {
                return Err(format!("{label} must be a canonical UUID"));
            }
        }
        if !valid_workspace_id(&self.workspace_id) {
            return Err("workspace ID must bind a canonical project, task, and source SHA".into());
        }
        if self.repository_id.is_empty()
            || self.repository_id.len() > 128
            || self.source_ref.is_empty()
            || self.source_ref.len() > 256
            || !valid_sha(&self.source_sha)
            || self.expected_task_revision.is_empty()
            || self.expected_task_revision.len() > 256
            || self.question.trim().is_empty()
            || self.question.chars().count() > MAX_QUESTION_CHARS
            || self.query.trim().is_empty()
            || self.query.chars().count() > 256
            || self.scope.paths.is_empty()
            || self.scope.paths.len() > MAX_SCOPE_PATHS
            || self.allowed_evidence_classes.is_empty()
            || self.allowed_evidence_classes.len() > 5
        {
            return Err("investigation request has an invalid identity, question, or scope".into());
        }
        self.budget.validate()?;
        let mut paths = std::collections::BTreeSet::new();
        for path in &self.scope.paths {
            if path.is_empty()
                || path.len() > MAX_PATH_CHARS
                || path.starts_with('/')
                || path.contains('\\')
                || path
                    .split('/')
                    .any(|part| part.is_empty() || part == "." || part == "..")
                || path.chars().any(char::is_control)
                || !paths.insert(path)
            {
                return Err("investigation scope contains an invalid or duplicate path".into());
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InvestigationOutcomeV1 {
    Findings,
    NotFoundInScope,
    InsufficientEvidence,
    ProviderUnavailable,
    StaleSource,
    ConflictingEvidence,
    Cancelled,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InvestigationSourceRefV1 {
    pub evidence_id: String,
    pub path: String,
    pub source_sha: String,
    pub content_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InvestigationReceiptV1 {
    pub schema_version: u32,
    pub idempotency_key: String,
    pub repository_id: String,
    pub source_sha: String,
    pub inspected_scope: Vec<String>,
    pub source_refs: Vec<InvestigationSourceRefV1>,
    pub findings: Vec<String>,
    pub uncertainty: Vec<String>,
    pub omitted_scope: Vec<String>,
    pub files_read: u16,
    pub bytes_read: u32,
    pub elapsed_ms: u32,
    /// Scope-bound confidence; this never turns partial evidence into absence.
    pub confidence: u8,
    pub outcome: InvestigationOutcomeV1,
}

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
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InvestigationEvidenceEnvelopeV1 {
    pub request: InvestigationRequestV1,
    pub receipt: InvestigationReceiptV1,
}

impl InvestigationAuthorizationV1 {
    pub fn context(&self) -> Result<super::InvestigationContext, String> {
        self.profile.validate()?;
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
        Ok(super::InvestigationContext {
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
        })
    }
}

pub(crate) fn valid_sha(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_workspace_id(value: &str) -> bool {
    let mut parts = value.split(':');
    let Some(project) = parts.next() else {
        return false;
    };
    let Some(task) = parts.next() else {
        return false;
    };
    let Some(sha) = parts.next() else {
        return false;
    };
    parts.next().is_none()
        && uuid::Uuid::parse_str(project).is_ok_and(|id| id.hyphenated().to_string() == project)
        && uuid::Uuid::parse_str(task).is_ok_and(|id| id.hyphenated().to_string() == task)
        && valid_sha(sha)
}
