use serde::{Deserialize, Serialize};

use super::{is_git_oid, is_sha256, is_uuid, valid_text};

const MAX_REQUIREMENTS: usize = 256;
const MAX_ACCEPTANCE: usize = 512;
const MAX_DEPENDENCY_CACHE_REFS: usize = 16;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QaPlatform {
    LinuxDesktop,
    HeadlessService,
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QaDependencyCacheRefV1 {
    pub dependency_request_id: String,
    pub cache_ref: crate::execution::contract::DependencyCacheRefV1,
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
    /// The exact operator-approved runtime target selected for this run.
    /// Optional only when reading historical records written before target binding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planning_item_uid: Option<String>,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependency_cache_refs: Vec<QaDependencyCacheRefV1>,
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
            || self
                .target_id
                .as_ref()
                .is_some_and(|target_id| !valid_text(target_id, 128))
            || self
                .planning_item_uid
                .as_ref()
                .is_some_and(|uid| !is_uuid(uid))
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
            || self.dependency_cache_refs.len() > MAX_DEPENDENCY_CACHE_REFS
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
        let mut dependency_arches = std::collections::BTreeSet::new();
        let mut dependency_arch_bytes = std::collections::BTreeMap::<&str, u64>::new();
        let mut dependency_arch_ids =
            std::collections::BTreeMap::<&str, std::collections::BTreeSet<&str>>::new();
        for dependency in &self.dependency_cache_refs {
            crate::execution::contract::validate_dependency_request_id(
                &dependency.dependency_request_id,
            )
            .map_err(|_| "runtime QA dependency request identity is invalid")?;
            dependency
                .cache_ref
                .validate()
                .map_err(|_| "runtime QA dependency cache reference is invalid")?;
            if self.planning_item_uid.as_deref() != Some(dependency.cache_ref.task_id.as_str()) {
                return Err("runtime QA dependency cache scope is invalid".into());
            }
            let architecture = crate::execution::docker::discovery::parse_architecture(
                &dependency.cache_ref.target_arch,
            )
            .ok_or("runtime QA dependency cache architecture is unsupported")?;
            if dependency.cache_ref.target_id != "local"
                || !dependency_arches
                    .insert((dependency.dependency_request_id.as_str(), architecture))
            {
                return Err("runtime QA dependency cache scope is invalid".into());
            }
            let architecture_bytes = dependency_arch_bytes.entry(architecture).or_default();
            *architecture_bytes =
                architecture_bytes.saturating_add(dependency.cache_ref.size_bytes);
            dependency_arch_ids
                .entry(architecture)
                .or_default()
                .insert(dependency.dependency_request_id.as_str());
        }
        if dependency_arch_bytes
            .values()
            .any(|bytes| *bytes > 128 * 1024 * 1024)
            || dependency_arch_ids
                .values()
                .any(|dependencies| dependencies.len() > 8)
        {
            return Err("runtime QA dependency caches exceed the transfer limit".into());
        }
        if !self.dependency_cache_refs.is_empty() && self.planning_item_uid.is_none() {
            return Err("runtime QA dependency cache planning identity is missing".into());
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
