use serde::{Deserialize, Serialize};

use crate::domain::ArtifactIdentity;

use super::content::{
    canonical_uuid, is_hash, list, section, sha256, snapshot_uid, stable_requirement_id,
};
use super::{content, hash};
mod validation;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanRequirementV1 {
    pub stable_id: String,
    pub text: String,
    #[serde(
        default,
        skip_serializing_if = "String::is_empty",
        deserialize_with = "deserialize_optional_string"
    )]
    pub rationale: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty", alias = "acIds")]
    pub acceptance_criteria_ids: Vec<String>,
}

fn deserialize_optional_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}

/// Immutable, application-parsed input to explicit plan confirmation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanSnapshotV1 {
    pub schema_version: u16,
    pub uid: String,
    pub feature_uid: String,
    pub feature_id: String,
    /// Stable product capability key, when this plan defines an installable capability.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_id: Option<String>,
    pub original_goal_ref: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub original_goal: String,
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

impl PlanSnapshotV1 {
    pub fn from_feature(
        feature_id: &str,
        identity: &ArtifactIdentity,
        markdown: &str,
        approval_contract: &str,
    ) -> Result<Self, String> {
        Self::build(feature_id, identity, markdown, approval_contract, None)
    }

    pub fn from_feature_with_original_goal(
        feature_id: &str,
        identity: &ArtifactIdentity,
        markdown: &str,
        approval_contract: &str,
        original_goal_ref: &str,
        original_goal: &str,
    ) -> Result<Self, String> {
        if original_goal_ref.trim().is_empty() || original_goal.trim().is_empty() {
            return Err("the original user goal and its reference are required".into());
        }
        Self::build(
            feature_id,
            identity,
            markdown,
            approval_contract,
            Some((original_goal_ref, original_goal)),
        )
    }

    fn build(
        feature_id: &str,
        identity: &ArtifactIdentity,
        markdown: &str,
        approval_contract: &str,
        original_goal: Option<(&str, &str)>,
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
            .map(|text| {
                let stable_id = stable_requirement_id(&text);
                let rationale = content::requirement_rationale(&text);
                let acceptance_criteria_ids = content::acceptance_criteria_refs(&text);
                PlanRequirementV1 {
                    stable_id,
                    text,
                    rationale,
                    acceptance_criteria_ids,
                }
            })
            .collect::<Vec<_>>();
        let acceptance_criteria = list(&section(markdown, "Acceptance Criteria"))
            .into_iter()
            .enumerate()
            .map(|(index, criterion)| validation::parse_acceptance_criterion(index, criterion))
            .collect::<Result<Vec<_>, _>>()?;
        let scope = list(&section(markdown, "Scope"));
        let capability_id = section(markdown, "Capability ID");
        let capability_id = (!capability_id.is_empty()).then_some(capability_id);
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
        let schema_version = if original_goal.is_some() {
            3
        } else if capability_id.is_some() {
            2
        } else {
            1
        };
        let spec_revision = match original_goal {
            Some((reference, goal)) => sha256(
                &serde_json::to_vec(&(approval_contract, reference, goal))
                    .map_err(|error| error.to_string())?,
            ),
            None => sha256(approval_contract.as_bytes()),
        };
        let mut snapshot = Self {
            schema_version,
            uid: String::new(),
            feature_uid: identity.uid.clone(),
            feature_id: feature_id.to_owned(),
            capability_id,
            original_goal_ref: original_goal.map_or_else(
                || format!("feature:{feature_id}"),
                |(reference, _)| reference.to_owned(),
            ),
            original_goal: original_goal.map_or_else(String::new, |(_, goal)| goal.trim().into()),
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
            deferred_assumptions: content::deferred_assumptions(markdown),
            decisions: section(markdown, "Decisions and Assumptions"),
            spec_revision,
            approval_contract_sha256: sha256(approval_contract.as_bytes()),
            content_hash: String::new(),
            author_run_id: None,
            author_session_id: None,
        };
        snapshot.content_hash = hash::content_hash(&snapshot)?;
        snapshot.uid = snapshot_uid(&snapshot.feature_uid, &snapshot.content_hash)?;
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), String> {
        if !matches!(self.schema_version, 1..=3)
            || !canonical_uuid(&self.uid)
            || !canonical_uuid(&self.feature_uid)
            || self.feature_id.trim().is_empty()
            || self.original_goal_ref.trim().is_empty()
            || self
                .capability_id
                .as_deref()
                .is_some_and(|value| !valid_capability_id(value))
            || (self.schema_version == 1 && self.capability_id.is_some())
            || (self.schema_version < 3 && !self.original_goal.is_empty())
            || (self.schema_version >= 3 && self.original_goal.trim().is_empty())
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
            || self.content_hash != hash::content_hash(self)?
        {
            return Err("plan snapshot is invalid or its content hash does not match".into());
        }
        if self.requirements.iter().any(|item| {
            item.stable_id.trim().is_empty()
                || item.text.trim().is_empty()
                || (self.schema_version >= 3 && item.rationale.trim().is_empty())
        }) || self.requirements.iter().enumerate().any(|(index, item)| {
            self.requirements[..index]
                .iter()
                .any(|prior| prior.stable_id == item.stable_id)
        }) {
            return Err("plan snapshot requirements need unique IDs and nonempty text".into());
        }
        validation::validate_acceptance_criteria(self)?;
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

    pub(crate) fn set_deferred_assumptions(
        &mut self,
        assumptions: Vec<String>,
    ) -> Result<(), String> {
        self.deferred_assumptions.extend(assumptions);
        self.deferred_assumptions.sort();
        self.deferred_assumptions.dedup();
        self.content_hash = hash::content_hash(self)?;
        self.uid = snapshot_uid(&self.feature_uid, &self.content_hash)?;
        self.validate()
    }

    pub(crate) fn set_recorded_decisions(&mut self, decisions: &[String]) -> Result<(), String> {
        if !decisions.is_empty() {
            self.decisions
                .push_str("\n\nPersisted planning decisions:\n");
            self.decisions.push_str(&decisions.join("\n"));
        }
        self.content_hash = hash::content_hash(self)?;
        self.uid = snapshot_uid(&self.feature_uid, &self.content_hash)?;
        self.validate()
    }
}

pub(super) fn valid_capability_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'.'
        })
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase())
}
