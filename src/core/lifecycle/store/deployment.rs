use serde::{Deserialize, Serialize};

use crate::artifacts::planning_store::{RecordRevisionCheck, StoreError};

use super::{CurrentImplementationEvidenceV1, LifecycleStore, records::now_unix_ms};

#[cfg(test)]
#[path = "deployment/tests.rs"]
mod tests;

/// Latest GitHub deployment status observed for the verified current commit.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeploymentEvidenceV1 {
    pub schema_version: u16,
    pub feature_uid: String,
    pub current_implementation_revision: u64,
    pub specification_revision: u64,
    pub specification_hash: String,
    pub merge_commit_sha: String,
    pub repository_id: String,
    pub deployment_id: Option<u64>,
    pub status_id: Option<u64>,
    pub environment: Option<String>,
    pub production_environment: bool,
    pub state: Option<String>,
    pub observed_at_ms: u64,
    pub revision: u64,
}

impl DeploymentEvidenceV1 {
    pub(crate) fn matches_current(&self, current: &CurrentImplementationEvidenceV1) -> bool {
        self.schema_version == 1
            && self.feature_uid == current.feature_uid
            && self.current_implementation_revision == current.revision
            && self.specification_revision == current.specification_revision
            && self.specification_hash == current.specification_hash
            && self.merge_commit_sha == current.merge_commit_sha
            && self.repository_id == current.repository_id
    }

    pub(crate) fn deployed_spec_hash(
        &self,
        current: &CurrentImplementationEvidenceV1,
    ) -> Option<&str> {
        (self.matches_current(current)
            && self.production_environment
            && self.state.as_deref() == Some("success"))
        .then_some(&self.specification_hash)
    }

    fn validate(&self) -> Result<(), StoreError> {
        let valid_sha = |value: &str| {
            matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
        };
        if self.schema_version != 1
            || !crate::core::lifecycle::types::is_uuid(&self.feature_uid)
            || self.current_implementation_revision == 0
            || self.specification_revision == 0
            || !crate::core::lifecycle::types::is_sha256(&self.specification_hash)
            || !valid_sha(&self.merge_commit_sha)
            || self.repository_id.trim().is_empty()
            || self.repository_id.len() > 80
            || self.deployment_id.is_some_and(|id| id == 0)
            || self.status_id.is_some_and(|id| id == 0)
            || self.environment.as_ref().is_some_and(|name| {
                name.trim().is_empty() || name.len() > 255 || name.chars().any(char::is_control)
            })
            || self.state.as_ref().is_some_and(|state| {
                !matches!(
                    state.as_str(),
                    "queued"
                        | "pending"
                        | "in_progress"
                        | "success"
                        | "failure"
                        | "error"
                        | "inactive"
                )
            })
            || self.observed_at_ms == 0
            || self.revision == 0
            || self.status_id.is_some() != self.state.is_some()
            || self.deployment_id.is_none() && self.state.is_some()
            || (self.state.is_some() && self.environment.is_none())
        {
            return Err(StoreError::MalformedState(
                "deployment evidence is malformed".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DeploymentProjection {
    schema_version: u16,
    feature_uid: String,
    created_at_ms: u64,
    updated_at_ms: u64,
    revision: u64,
    record: DeploymentEvidenceV1,
}

impl LifecycleStore<'_> {
    pub(crate) fn validate_deployment_repository(
        &self,
        current: &CurrentImplementationEvidenceV1,
        repository: &std::path::Path,
        provider_url: &str,
        provider_pr_id: u64,
    ) -> Result<(), StoreError> {
        super::current_implementation::origin::validate(
            self.store,
            repository,
            &current.repository_id,
            provider_url,
            provider_pr_id,
        )
    }

    pub(crate) fn load_deployment_evidence(
        &self,
        feature_uid: &str,
    ) -> Result<Option<DeploymentEvidenceV1>, StoreError> {
        if !crate::core::lifecycle::types::is_uuid(feature_uid) {
            return Err(StoreError::MalformedState("feature UID is invalid".into()));
        }
        let path = format!("state/deployments/{feature_uid}.json");
        match self.store.read_record::<DeploymentProjection>(&path) {
            Ok((projection, actual))
                if projection.schema_version == 1
                    && projection.feature_uid == feature_uid
                    && projection.created_at_ms > 0
                    && projection.updated_at_ms >= projection.created_at_ms
                    && projection.revision == actual
                    && projection.record.revision == actual =>
            {
                projection.record.validate()?;
                Ok(Some(projection.record))
            }
            Ok(_) => Err(StoreError::MalformedState(
                "deployment record identity is inconsistent".into(),
            )),
            Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    pub(crate) fn record_deployment_evidence(
        &self,
        current: &CurrentImplementationEvidenceV1,
        observation: DeploymentObservation,
    ) -> Result<DeploymentEvidenceV1, StoreError> {
        let current_path = format!("state/current_implementations/{}.json", current.feature_uid);
        let (persisted_current, current_revision) =
            self.store
                .read_record::<super::current_implementation::CurrentProjection>(&current_path)?;
        if persisted_current.schema_version != 1
            || persisted_current.uid != current.feature_uid
            || persisted_current.created_at_ms == 0
            || persisted_current.updated_at_ms < persisted_current.created_at_ms
            || persisted_current.revision != current_revision
            || persisted_current.record != *current
        {
            return Err(StoreError::MalformedState(
                "deployment observation does not match the current implementation receipt".into(),
            ));
        }
        let path = format!("state/deployments/{}.json", current.feature_uid);
        let existing = match self.store.read_record::<DeploymentProjection>(&path) {
            Ok((projection, actual))
                if projection.schema_version == 1
                    && projection.feature_uid == current.feature_uid
                    && projection.created_at_ms > 0
                    && projection.updated_at_ms >= projection.created_at_ms
                    && projection.revision == actual
                    && projection.record.revision == actual =>
            {
                projection.record.validate()?;
                Some((projection, actual))
            }
            Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
                None
            }
            Ok(_) => {
                return Err(StoreError::MalformedState(
                    "deployment record identity is inconsistent".into(),
                ));
            }
            Err(error) => return Err(error),
        };
        if let Some((projection, deployment_revision)) = existing.as_ref()
            && matches_observation(&projection.record, current, &observation)
        {
            self.store.transaction_with_record_revisions(
                &[],
                &[
                    RecordRevisionCheck {
                        path,
                        expected_revision: *deployment_revision,
                    },
                    RecordRevisionCheck {
                        path: current_path,
                        expected_revision: current_revision,
                    },
                ],
            )?;
            return Ok(projection.record.clone());
        }
        let (revision, created_at) = existing
            .map(|(projection, revision)| (revision, projection.created_at_ms))
            .unwrap_or_else(|| (0, now_unix_ms()));
        let next_revision = revision
            .checked_add(1)
            .ok_or_else(|| StoreError::MalformedState("deployment revision exhausted".into()))?;
        let record = DeploymentEvidenceV1 {
            schema_version: 1,
            feature_uid: current.feature_uid.clone(),
            current_implementation_revision: current.revision,
            specification_revision: current.specification_revision,
            specification_hash: current.specification_hash.clone(),
            merge_commit_sha: current.merge_commit_sha.clone(),
            repository_id: current.repository_id.clone(),
            deployment_id: observation.deployment_id,
            status_id: observation.status_id,
            environment: observation.environment,
            production_environment: observation.production_environment,
            state: observation.state,
            observed_at_ms: now_unix_ms(),
            revision: next_revision,
        };
        record.validate()?;
        let projection = DeploymentProjection {
            schema_version: 1,
            feature_uid: current.feature_uid.clone(),
            created_at_ms: created_at,
            updated_at_ms: record.observed_at_ms,
            revision: next_revision,
            record: record.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&projection)
            .map_err(|error| StoreError::MalformedState(error.to_string()))?;
        self.store.transaction_with_record_revisions(
            &[(path.clone(), bytes)],
            &[
                RecordRevisionCheck {
                    path,
                    expected_revision: revision,
                },
                RecordRevisionCheck {
                    path: current_path,
                    expected_revision: current_revision,
                },
            ],
        )?;
        Ok(record)
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct DeploymentObservation {
    pub deployment_id: Option<u64>,
    pub status_id: Option<u64>,
    pub environment: Option<String>,
    pub production_environment: bool,
    pub state: Option<String>,
}

fn matches_observation(
    record: &DeploymentEvidenceV1,
    current: &CurrentImplementationEvidenceV1,
    observation: &DeploymentObservation,
) -> bool {
    record.matches_current(current)
        && record.deployment_id == observation.deployment_id
        && record.status_id == observation.status_id
        && record.environment == observation.environment
        && record.production_environment == observation.production_environment
        && record.state == observation.state
}
