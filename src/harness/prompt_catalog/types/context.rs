use serde::Serialize;

use super::{BootstrapConversationPayloadV1, PromptRole};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceOrigin {
    User,
    Repository,
    Tool,
    Application,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceConfidentiality {
    Public,
    WorkspacePrivate,
    Restricted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EvidenceItem {
    pub id: String,
    pub workspace_uid: String,
    pub origin: EvidenceOrigin,
    pub confidentiality: EvidenceConfidentiality,
    pub source: String,
    pub content: String,
}

/// Typed context common to every prompt. Dynamic repository and user material is
/// serialized only into the user message, after the trusted role and policy.
#[derive(Debug, Clone, Serialize)]
pub struct RoleInputV1 {
    pub role: PromptRole,
    pub workspace_uid: String,
    pub feature_uid: String,
    pub task_uid: Option<String>,
    pub original_goal: String,
    pub approved_spec_ref: Option<String>,
    pub approved_spec_hash: Option<String>,
    pub decision_refs: Vec<String>,
    pub evidence_manifest: Vec<EvidenceItem>,
    pub execution_policy_ref: String,
    pub output_schema_version: String,
    pub payload: RolePayloadV1,
}

impl RoleInputV1 {
    /// Build the stable no-workspace envelope used for machine setup dialogue.
    pub fn bootstrap_conversation(payload: BootstrapConversationPayloadV1) -> Self {
        let role = PromptRole::BootstrapConversation;
        Self {
            role,
            workspace_uid: "machine-setup".into(),
            feature_uid: "bootstrap-conversation".into(),
            task_uid: None,
            original_goal: "Help the user understand this machine setup prerequisite.".into(),
            approved_spec_ref: None,
            approved_spec_hash: None,
            decision_refs: Vec::new(),
            evidence_manifest: Vec::new(),
            execution_policy_ref: "bootstrap_conversation/v1".into(),
            output_schema_version: role.output_schema_version().into(),
            payload: RolePayloadV1::BootstrapConversation(payload),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "request", rename_all = "snake_case")]
pub enum RolePayloadV1 {
    Manager {
        board_event_cursor: String,
        work_graph: Vec<String>,
        unresolved_findings: Vec<String>,
        latest_review_receipts: Vec<String>,
    },
    Planner {
        open_questions: Vec<String>,
        previous_answers: Vec<String>,
        research_evidence: Vec<String>,
        requested_scope: String,
    },
    PlannerOpportunity {
        open_questions: Vec<String>,
        previous_answers: Vec<String>,
        research_evidence: Vec<String>,
        requested_scope: String,
        opportunity_origin: Option<String>,
        existing_support_evidence: Vec<String>,
        workaround: Option<String>,
        user_value: Option<String>,
        confidentiality_class: EvidenceConfidentiality,
        concrete_target_repo: Option<String>,
    },
    PlannerTurn {
        turn_context: String,
        workflow_instructions: Option<String>,
        operator_persona: String,
        task_mode_note: Option<String>,
    },
    Review {
        subject_revision: String,
        subject_hash: String,
        author_run_id: String,
        reviewer_run_id: String,
        author_session_id: Option<String>,
        reviewer_session_id: Option<String>,
        requirements_matrix: Vec<String>,
        explicit_exclusions: Vec<String>,
    },
    QaScenarioSynthesis {
        immutable_request_json: String,
    },
    Decomposer {
        approved_plan_hash: String,
        approved_scope: String,
    },
    Worker {
        assigned_task: String,
        assigned_subtasks: Vec<String>,
        immutable_baseline: String,
        allowed_execution_tools: Vec<String>,
    },
    SpecSync {
        approved_requirements: Vec<String>,
        verified_implementation_refs: Vec<String>,
    },
    Repair {
        assigned_blocking_findings: Vec<String>,
        approved_task_baseline: String,
        allowed_execution_tools: Vec<String>,
    },
    Pr {
        reviewed_task_refs: Vec<String>,
        approved_subtasks: Vec<String>,
        base_ref: String,
        head_ref: String,
    },
    BootstrapConversation(BootstrapConversationPayloadV1),
}
