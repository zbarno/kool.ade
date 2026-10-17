use serde::{Deserialize, Serialize};

use crate::domain::ArtifactIdentity;

mod author;
mod confirmation;
mod persistence;
#[cfg(not(test))]
pub(crate) use persistence::recorded_review_decision;
pub(crate) use persistence::{
    from_feature_with_author, has_approved_review, has_current_user_confirmation,
    has_user_confirmation, is_persisted, load_persisted, plan_review_receipt, record_author,
    record_change, review_change, user_confirmation_change, user_confirmation_receipt,
};

pub(crate) fn confirmed_product_spec_hash(
    receipt: &crate::core::lifecycle::GateReceipt,
) -> Option<&str> {
    confirmation::confirmed_product_spec_hash(receipt)
}
mod content;
mod store;
use content::{
    canonical_uuid, is_hash, list, section, sha256, snapshot_uid, stable_requirement_id,
};

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanAuthorProvenanceV1 {
    pub feature_uid: String,
    pub approval_contract_sha256: String,
    pub author_run_id: String,
    pub author_session_id: String,
}

#[cfg(test)]
#[path = "plan_snapshot/tests.rs"]
mod tests;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanRequirementV1 {
    pub stable_id: String,
    pub text: String,
}

/// Immutable, application-parsed input to explicit plan confirmation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanSnapshotV1 {
    pub schema_version: u16,
    pub uid: String,
    pub feature_uid: String,
    pub feature_id: String,
    pub original_goal_ref: String,
    pub intent: String,
    pub outcome: String,
    pub current_state: String,
    pub target_behavior: String,
    pub scope: Vec<String>,
    pub non_goals: Vec<String>,
    pub requirements: Vec<PlanRequirementV1>,
    pub acceptance_criteria: Vec<String>,
    pub architecture: String,
    pub dependencies: Vec<String>,
    pub risks: Vec<String>,
    pub security_constraints: Vec<String>,
    pub test_strategy: String,
    pub deployment_notes: String,
    pub open_questions: Vec<String>,
    pub deferred_assumptions: Vec<String>,
    pub decisions: String,
    pub spec_revision: String,
    pub approval_contract_sha256: String,
    pub content_hash: String,
    /// Persisted identity of the run/session that authored this confirmed plan.
    /// Missing provenance is valid legacy data but blocks independent review.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_session_id: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotContent<'a> {
    schema_version: u16,
    feature_uid: &'a str,
    feature_id: &'a str,
    original_goal_ref: &'a str,
    intent: &'a str,
    outcome: &'a str,
    current_state: &'a str,
    target_behavior: &'a str,
    scope: &'a [String],
    non_goals: &'a [String],
    requirements: &'a [PlanRequirementV1],
    acceptance_criteria: &'a [String],
    architecture: &'a str,
    dependencies: &'a [String],
    risks: &'a [String],
    security_constraints: &'a [String],
    test_strategy: &'a str,
    deployment_notes: &'a str,
    open_questions: &'a [String],
    deferred_assumptions: &'a [String],
    decisions: &'a str,
    spec_revision: &'a str,
    approval_contract_sha256: &'a str,
}

impl PlanSnapshotV1 {
    pub fn from_feature(
        feature_id: &str,
        identity: &ArtifactIdentity,
        markdown: &str,
        approval_contract: &str,
    ) -> Result<Self, String> {
        if !canonical_uuid(&identity.uid)
            || identity.display_id.trim().is_empty()
            || identity.title.trim().is_empty()
            || identity
                .parent_uid
                .as_deref()
                .is_some_and(|uid| !canonical_uuid(uid))
        {
            return Err("feature identity is invalid".into());
        }
        let requirements = list(&section(markdown, "Requirements"))
            .into_iter()
            .map(|text| PlanRequirementV1 {
                stable_id: stable_requirement_id(&text),
                text,
            })
            .collect::<Vec<_>>();
        let acceptance_criteria = list(&section(markdown, "Acceptance Criteria"));
        let scope = list(&section(markdown, "Scope"));
        let non_goals = scope
            .iter()
            .filter(|item| {
                let normalized = item.to_ascii_lowercase();
                normalized.starts_with("out of scope:")
                    || normalized.starts_with("not in scope:")
                    || normalized.starts_with("non-goal:")
            })
            .cloned()
            .collect();
        let mut snapshot = Self {
            schema_version: 1,
            uid: String::new(),
            feature_uid: identity.uid.clone(),
            feature_id: feature_id.to_owned(),
            original_goal_ref: format!("feature:{feature_id}"),
            intent: section(markdown, "Intent"),
            outcome: section(markdown, "Desired Behavior"),
            current_state: section(markdown, "Current Behavior"),
            target_behavior: section(markdown, "Desired Behavior"),
            scope,
            non_goals,
            requirements,
            acceptance_criteria,
            architecture: section(markdown, "Affected Product Areas"),
            dependencies: list(&section(markdown, "Dependencies")),
            risks: list(&section(markdown, "Risks")),
            security_constraints: list(&section(markdown, "Security Constraints")),
            test_strategy: section(markdown, "Test Strategy"),
            deployment_notes: section(markdown, "Deployment Notes"),
            open_questions: list(&section(markdown, "Open Questions")),
            deferred_assumptions: Vec::new(),
            decisions: section(markdown, "Decisions and Assumptions"),
            spec_revision: sha256(approval_contract.as_bytes()),
            approval_contract_sha256: sha256(approval_contract.as_bytes()),
            content_hash: String::new(),
            author_run_id: None,
            author_session_id: None,
        };
        snapshot.content_hash = snapshot.compute_content_hash()?;
        snapshot.uid = snapshot_uid(&snapshot.feature_uid, &snapshot.content_hash)?;
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1
            || !canonical_uuid(&self.uid)
            || !canonical_uuid(&self.feature_uid)
            || self.feature_id.trim().is_empty()
            || self.original_goal_ref.trim().is_empty()
            || self.intent.trim().is_empty()
            || self.outcome.trim().is_empty()
            || self.current_state.trim().is_empty()
            || self.target_behavior.trim().is_empty()
            || self.scope.is_empty()
            || self.requirements.is_empty()
            || self.acceptance_criteria.is_empty()
            || !is_hash(&self.content_hash)
            || !is_hash(&self.spec_revision)
            || !is_hash(&self.approval_contract_sha256)
            || self.uid != snapshot_uid(&self.feature_uid, &self.content_hash)?
            || self.content_hash != self.compute_content_hash()?
        {
            return Err("plan snapshot is invalid or its content hash does not match".into());
        }
        if self
            .requirements
            .iter()
            .any(|item| item.stable_id.trim().is_empty() || item.text.trim().is_empty())
            || self.requirements.iter().enumerate().any(|(index, item)| {
                self.requirements[..index]
                    .iter()
                    .any(|prior| prior.stable_id == item.stable_id)
            })
        {
            return Err("plan snapshot requirements need unique IDs and nonempty text".into());
        }
        if self.author_run_id.is_some() != self.author_session_id.is_some()
            || self
                .author_run_id
                .as_ref()
                .is_some_and(|id| !canonical_uuid(id))
            || self
                .author_session_id
                .as_ref()
                .is_some_and(|id| !canonical_uuid(id))
        {
            return Err("plan author provenance must include valid run and session IDs".into());
        }
        Ok(())
    }

    fn compute_content_hash(&self) -> Result<String, String> {
        let content = SnapshotContent {
            schema_version: self.schema_version,
            feature_uid: &self.feature_uid,
            feature_id: &self.feature_id,
            original_goal_ref: &self.original_goal_ref,
            intent: &self.intent,
            outcome: &self.outcome,
            current_state: &self.current_state,
            target_behavior: &self.target_behavior,
            scope: &self.scope,
            non_goals: &self.non_goals,
            requirements: &self.requirements,
            acceptance_criteria: &self.acceptance_criteria,
            architecture: &self.architecture,
            dependencies: &self.dependencies,
            risks: &self.risks,
            security_constraints: &self.security_constraints,
            test_strategy: &self.test_strategy,
            deployment_notes: &self.deployment_notes,
            open_questions: &self.open_questions,
            deferred_assumptions: &self.deferred_assumptions,
            decisions: &self.decisions,
            spec_revision: &self.spec_revision,
            approval_contract_sha256: &self.approval_contract_sha256,
        };
        serde_json::to_vec(&content)
            .map(|bytes| sha256(&bytes))
            .map_err(|error| error.to_string())
    }
}
