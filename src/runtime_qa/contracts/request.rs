use serde::{Deserialize, Serialize};

use super::{is_git_oid, is_sha256, is_uuid, valid_text};

const MAX_REQUIREMENTS: usize = 256;
const MAX_ACCEPTANCE: usize = 512;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QaPlatform {
    LinuxDesktop,
    Web,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QaBoundedLimitsV1 {
    pub max_scenarios: u16,
    pub max_actions_per_scenario: u16,
    pub max_total_actions: u32,
    pub timeout_seconds: u32,
    pub max_evidence_bytes: u64,
    pub max_screenshots: u16,
}

impl QaBoundedLimitsV1 {
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=256).contains(&self.max_scenarios)
            || !(1..=256).contains(&self.max_actions_per_scenario)
            || !(1..=16_384).contains(&self.max_total_actions)
            || !(1..=7_200).contains(&self.timeout_seconds)
            || !(1..=2 * 1024 * 1024 * 1024).contains(&self.max_evidence_bytes)
            || !(1..=2_048).contains(&self.max_screenshots)
        {
            return Err("runtime QA bounds exceed supported limits".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QaRunRequestV1 {
    pub schema_version: u16,
    pub qa_run_id: String,
    pub parent_task_uid: String,
    pub feature_uid: String,
    pub code_head_sha: String,
    /// Hash of the immutable application artifact produced by the approved build.
    pub build_artifact_sha256: String,
    pub complete_diff_hash: String,
    pub review_subject_sha256: String,
    pub approved_spec_hash: String,
    pub requirement_ids: Vec<String>,
    pub acceptance_criteria: Vec<String>,
    pub relevant_platforms: Vec<QaPlatform>,
    pub startup_recipe_ref: String,
    pub startup_recipe_sha256: String,
    pub execution_session_id: String,
    pub driver_profile_version: String,
    pub test_data_fixture_ref: Option<String>,
    pub bounded_limits: QaBoundedLimitsV1,
    /// Captured from the implementation session so a separate reviewer can be proven.
    pub author_run_id: String,
    pub author_session_id: String,
}

impl QaRunRequestV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1
            || !is_uuid(&self.qa_run_id)
            || !is_uuid(&self.parent_task_uid)
            || !is_uuid(&self.feature_uid)
            || !is_git_oid(&self.code_head_sha)
            || !is_sha256(&self.build_artifact_sha256)
            || !is_sha256(&self.complete_diff_hash)
            || !is_sha256(&self.review_subject_sha256)
            || !is_sha256(&self.approved_spec_hash)
            || self.requirement_ids.is_empty()
            || self.requirement_ids.len() > MAX_REQUIREMENTS
            || self.acceptance_criteria.is_empty()
            || self.acceptance_criteria.len() > MAX_ACCEPTANCE
            || self.acceptance_criteria.len() != self.requirement_ids.len()
            || self.relevant_platforms.is_empty()
            || self.relevant_platforms.len() > 2
            || has_duplicates(&self.relevant_platforms)
            || !valid_text(&self.startup_recipe_ref, 128)
            || !is_sha256(&self.startup_recipe_sha256)
            || !is_uuid(&self.execution_session_id)
            || !valid_text(&self.driver_profile_version, 128)
            || self
                .test_data_fixture_ref
                .as_ref()
                .is_some_and(|value| !valid_text(value, 512))
            || !is_uuid(&self.author_run_id)
            || !is_uuid(&self.author_session_id)
            || self
                .requirement_ids
                .iter()
                .any(|value| !valid_text(value, 128))
            || has_duplicates(&self.requirement_ids)
            || self
                .acceptance_criteria
                .iter()
                .any(|value| !valid_text(value, 8192))
        {
            return Err("runtime QA request is invalid or exceeds limits".into());
        }
        self.bounded_limits.validate()
    }
}

fn has_duplicates<T: Ord>(items: &[T]) -> bool {
    items
        .iter()
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        != items.len()
}
