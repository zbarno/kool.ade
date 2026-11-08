use serde::{Deserialize, Serialize};
mod authorization;
pub use authorization::InvestigationAuthorizationV1;

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comparison: Option<InvestigationRevisionV1>,
}

/// A second revision may be compared only when the app includes its exact
/// approved repository pin in the authorization material.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InvestigationRevisionV1 {
    pub source_ref: String,
    pub source_sha: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InvestigationApprovedRevisionV1 {
    pub intent: crate::core::git_intent::GitIntentV1,
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
        if let Some(comparison) = &self.comparison
            && (comparison.source_ref.is_empty()
                || comparison.source_ref.len() > 256
                || !comparison.source_ref.starts_with("refs/heads/")
                || !valid_sha(&comparison.source_sha)
                || (comparison.source_ref == self.source_ref
                    && comparison.source_sha == self.source_sha)
                || self.budget.max_files < 2)
        {
            return Err("comparison revision has an invalid or duplicate identity".into());
        }
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
pub struct InvestigationComparisonRefV1 {
    pub evidence_id: String,
    pub path: String,
    pub left_source_sha: String,
    pub left_content_sha256: String,
    pub right_source_sha: String,
    pub right_content_sha256: String,
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
    #[serde(default)]
    pub comparison_refs: Vec<InvestigationComparisonRefV1>,
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InvestigationEvidenceEnvelopeV1 {
    pub request: InvestigationRequestV1,
    pub receipt: InvestigationReceiptV1,
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
