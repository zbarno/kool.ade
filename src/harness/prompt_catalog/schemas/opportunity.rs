use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ManagerOutputV1 {
    pub summary: String,
    pub proposed_actions: Vec<ManagerActionV1>,
    #[serde(default)]
    pub opportunities: Vec<OpportunityProposalV1Legacy>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ManagerOutputV2 {
    pub summary: String,
    pub proposed_actions: Vec<ManagerActionV1>,
    #[serde(default)]
    pub opportunities: Vec<OpportunityProposalV1>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ManagerActionV1 {
    pub kind: ManagerActionKind,
    pub target: String,
    pub reason: String,
    pub evidence: Vec<String>,
    pub expected_revision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contradiction: Option<ManagerContradictionV1>,
}

/// Explicit claim-to-source mapping required when a Manager contests a review
/// finding. The source quote is checked against the cited evidence verbatim.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ManagerContradictionV1 {
    pub finding_claim: String,
    pub source_evidence_id: String,
    pub source_quote: String,
    pub explanation: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagerActionKind {
    RepairPlan,
    Investigate,
    AskUser,
    PromoteFollowUp,
    ContestFinding,
    MaintainSpecification,
    CoordinateReview,
    RepairFinding,
    PreparePr,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OpportunityProposalV1 {
    pub origin: OpportunityOrigin,
    pub kind: OpportunityKind,
    pub target: OpportunityTarget,
    pub target_repository: String,
    pub target_repository_ref: String,
    pub related_feature_uids: Vec<String>,
    pub related_task_uids: Vec<String>,
    pub observation_revision: String,
    pub evidence_refs: Vec<String>,
    pub problem_or_opportunity: String,
    pub user_value: String,
    pub expected_outcome: String,
    pub possible_approaches: Vec<String>,
    pub recommended_approach: String,
    pub alternatives: String,
    pub estimated_scope: OpportunityScope,
    pub impact: String,
    pub cost_or_risk: String,
    pub affected_systems: Vec<String>,
    pub current_capability_state: String,
    pub generic_workaround: Option<String>,
    pub proposed_approach: String,
    pub dedup_fingerprint: String,
    pub inference: String,
    pub confidence: OpportunityConfidence,
    pub speculative: bool,
    pub generic_capability_check: String,
    pub deduplication_basis: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OpportunityProposalV1Legacy {
    pub origin: OpportunityOrigin,
    pub kind: OpportunityKind,
    pub target: OpportunityTarget,
    pub target_repository: String,
    pub target_repository_ref: String,
    pub related_feature_uids: Vec<String>,
    pub related_task_uids: Vec<String>,
    pub observation_revision: String,
    pub evidence_refs: Vec<String>,
    pub problem_or_opportunity: String,
    pub user_value: String,
    pub expected_outcome: String,
    pub possible_approaches: Vec<String>,
    pub recommended_approach: String,
    pub alternatives: String,
    pub estimated_scope: OpportunityScope,
    pub impact: String,
    pub cost_or_risk: String,
    pub affected_systems: Vec<String>,
    pub current_capability_state: String,
    pub generic_workaround: Option<String>,
    pub proposed_approach: String,
    pub dedup_fingerprint: String,
    pub inference: String,
    pub speculative: bool,
    pub generic_capability_check: String,
    pub deduplication_basis: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpportunityOrigin {
    UserGoal,
    RunFailure,
    QaFinding,
    ReviewFinding,
    SpecDrift,
    CodeInspection,
    UserFeedback,
    Roadmap,
    ScheduledExploration,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpportunityKind {
    NewFeature,
    ExistingFeatureExtension,
    BugFix,
    Integration,
    TechStackSupport,
    Quality,
    Security,
    Performance,
    DeveloperExperience,
    Documentation,
    Infrastructure,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpportunityTarget {
    ActiveUserProject,
    KooladeProduct,
    OtherLinkedRepository,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpportunityScope {
    Small,
    Medium,
    Large,
    Unknown,
}
#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpportunityConfidence {
    Low,
    Medium,
    High,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OpportunityDecisionV1 {
    pub uid: String,
    pub opportunity_uid: String,
    pub revision_hash: String,
    pub user_id: String,
    pub decision: OpportunityChoice,
    pub occurred_at: String,
    #[serde(default)]
    pub snooze_until: Option<String>,
    pub explanation: Option<String>,
}

/// Validate a UI-created decision against the authenticated user and exact
/// proposal revision. This is not used to deserialize model output.
pub fn validate_user_opportunity_decision(
    decision: &OpportunityDecisionV1,
    expected_opportunity_uid: &str,
    expected_revision_hash: &str,
    authenticated_user_id: &str,
) -> Result<(), String> {
    let valid_uuid = |value: &str| {
        uuid::Uuid::parse_str(value).is_ok_and(|parsed| parsed.hyphenated().to_string() == value)
    };
    let valid_hash =
        |value: &str| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit());
    if !valid_uuid(&decision.uid)
        || !valid_uuid(expected_opportunity_uid)
        || decision.opportunity_uid != expected_opportunity_uid
        || !valid_hash(expected_revision_hash)
        || decision.revision_hash != expected_revision_hash
        || authenticated_user_id.trim().is_empty()
        || authenticated_user_id.len() > 256
        || decision.user_id != authenticated_user_id
        || chrono::DateTime::parse_from_rfc3339(&decision.occurred_at).is_err()
        || match (&decision.decision, &decision.snooze_until) {
            (OpportunityChoice::NotNow, Some(until)) => {
                let start = chrono::DateTime::parse_from_rfc3339(&decision.occurred_at)
                    .map(|value| value.timestamp_millis());
                let end = chrono::DateTime::parse_from_rfc3339(until)
                    .map(|value| value.timestamp_millis());
                !matches!((start, end), (Ok(start), Ok(end)) if end > start && end <= start.saturating_add(90 * 24 * 60 * 60 * 1000))
            }
            (OpportunityChoice::NotNow, None) => true,
            (_, Some(_)) => true,
            (_, None) => false,
        }
        || decision
            .explanation
            .as_ref()
            .is_some_and(|value| value.len() > 4096 || value.contains('\0'))
    {
        return Err("opportunity decision does not match the authenticated user and current proposal revision".into());
    }
    Ok(())
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
#[derive(PartialEq, Eq)]
pub enum OpportunityChoice {
    Pursue,
    NotNow,
    Decline,
    AskForMoreInformation,
}
