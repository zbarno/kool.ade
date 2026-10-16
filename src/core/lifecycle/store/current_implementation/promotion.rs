use crate::artifacts::planning_store::{RecordRevisionCheck, StoreError};

use super::super::{
    LifecycleStore, records::now_unix_ms, task_transition::PullRequestPreparationStatus,
};
use super::{CurrentImplementationEvidenceV1, CurrentProjection, origin};

mod sources;

impl<'a> LifecycleStore<'a> {
    /// Promote a PR merge only after resolving the exact task/PR relationship
    /// and verifying PR-head -> merge -> checked-out HEAD ancestry in Git.
    pub fn record_verified_current_implementation(
        &self,
        task_uid: &str,
        repository_id: &str,
        repository: &std::path::Path,
    ) -> Result<CurrentImplementationEvidenceV1, StoreError> {
        self.record_verified_current_implementation_inner(task_uid, repository_id, repository, true)
    }

    #[cfg(test)]
    pub(crate) fn record_verified_current_implementation_for_local_fixture(
        &self,
        task_uid: &str,
        repository_id: &str,
        repository: &std::path::Path,
    ) -> Result<CurrentImplementationEvidenceV1, StoreError> {
        self.record_verified_current_implementation_inner(
            task_uid,
            repository_id,
            repository,
            false,
        )
    }

    fn record_verified_current_implementation_inner(
        &self,
        task_uid: &str,
        repository_id: &str,
        repository: &std::path::Path,
        validate_origin: bool,
    ) -> Result<CurrentImplementationEvidenceV1, StoreError> {
        self.record_verified_current_implementation_with_precommit(
            task_uid,
            repository_id,
            repository,
            validate_origin,
            || {},
        )
    }

    #[cfg(test)]
    pub(crate) fn record_verified_current_implementation_for_local_fixture_with_precommit(
        &self,
        task_uid: &str,
        repository_id: &str,
        repository: &std::path::Path,
        before_commit: impl FnOnce(),
    ) -> Result<CurrentImplementationEvidenceV1, StoreError> {
        self.record_verified_current_implementation_with_precommit(
            task_uid,
            repository_id,
            repository,
            false,
            before_commit,
        )
    }

    fn record_verified_current_implementation_with_precommit(
        &self,
        task_uid: &str,
        repository_id: &str,
        repository: &std::path::Path,
        validate_origin: bool,
        before_commit: impl FnOnce(),
    ) -> Result<CurrentImplementationEvidenceV1, StoreError> {
        let sources = sources::load(self, task_uid)?;
        let sources::CurrentImplementationSources {
            task,
            work,
            specification,
            checks: source_checks,
        } = sources;
        if task.phase != crate::core::lifecycle::TaskPhase::Done
            || work.task_uid != task.task_uid
            || work.feature_uid != task.feature_uid
            || !work
                .included_task_uids
                .iter()
                .any(|included| included == task_uid)
            || work.status != PullRequestPreparationStatus::Merged
        {
            return Err(StoreError::MalformedState(
                "current implementation source task or PR is not a validated merged task".into(),
            ));
        }
        let pr_head = work.head_sha.as_deref().ok_or_else(|| {
            StoreError::MalformedState("merged PR has no recorded head SHA".into())
        })?;
        let base = work.base_sha.as_deref().ok_or_else(|| {
            StoreError::MalformedState("merged PR has no recorded base SHA".into())
        })?;
        let merge = work.merged_commit_sha.as_deref().ok_or_else(|| {
            StoreError::MalformedState("merged PR has no provider merge SHA".into())
        })?;
        if task.reviewed_code_head_sha.as_deref() != Some(pr_head) {
            return Err(StoreError::MalformedState(
                "merged PR head does not match the task's independently reviewed commit".into(),
            ));
        }
        let provider_pr_id = work.provider_pr_id.ok_or_else(|| {
            StoreError::MalformedState("merged PR has no persisted provider number".into())
        })?;
        if validate_origin {
            let provider_url = work.provider_pr_url.as_deref().ok_or_else(|| {
                StoreError::MalformedState("merged PR has no persisted provider URL".into())
            })?;
            origin::validate(
                self.store,
                repository,
                repository_id,
                provider_url,
                provider_pr_id,
            )?;
        }
        crate::core::reconciliation::merge_verification::fetch_pull_request_head(
            repository,
            provider_pr_id,
            pr_head,
        )
        .map_err(|_| {
            StoreError::MalformedState(
                "provider PR head ref is unavailable or differs from the reviewed commit".into(),
            )
        })?;
        let proof = match crate::core::reconciliation::merge_verification::validate_merged_checkout(
            repository, base, pr_head, merge,
        ) {
            Ok(proof) => proof,
            Err(_) => {
                if crate::core::reconciliation::merge_verification::fetch_origin_with_deadline(
                    repository,
                )
                .is_err()
                {
                    return Err(StoreError::MalformedState(
                        "provider merge evidence is unavailable in the target repository".into(),
                    ));
                }
                crate::core::reconciliation::merge_verification::validate_merged_checkout(
                    repository, base, pr_head, merge,
                )
                .map_err(|_| {
                    StoreError::MalformedState(
                        "provider merge evidence does not match checked-out repository ancestry"
                            .into(),
                    )
                })?
            }
        };
        let current_path = format!("state/current_implementations/{}.json", task.feature_uid);
        let (current, current_revision) = match self
            .store
            .read_record::<CurrentProjection>(&current_path)
        {
            Ok((projection, record_revision)) => {
                if projection.uid != task.feature_uid
                    || projection.created_at_ms == 0
                    || projection.updated_at_ms < projection.created_at_ms
                    || projection.revision != record_revision
                {
                    return Err(StoreError::MalformedState(
                        "current implementation revision is inconsistent".into(),
                    ));
                }
                (Some(projection.record), record_revision)
            }
            Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
                (None, 0)
            }
            Err(error) => return Err(error),
        };
        if let Some(existing) = current.as_ref()
            && existing.task_uid == task_uid
            && existing.merge_commit_sha == proof.merge_commit
            && existing.checked_out_head_sha == proof.checked_out_head
            && existing.specification_revision == specification.seq
            && existing.specification_hash == specification.content_hash
        {
            self.store
                .transaction_with_record_revisions(&[], &source_checks)?;
            return Ok(existing.clone());
        }
        let revision = current_revision.checked_add(1).ok_or_else(|| {
            StoreError::MalformedState("current implementation revision is exhausted".into())
        })?;
        let now = now_unix_ms();
        let record = CurrentImplementationEvidenceV1 {
            schema_version: 1,
            uid: task.feature_uid.clone(),
            feature_uid: task.feature_uid.clone(),
            task_uid: task_uid.to_owned(),
            work_item_uid: work.work_item_uid,
            repository_id: repository_id.to_owned(),
            base_commit_sha: proof.base_commit,
            pull_request_head_sha: proof.pull_request_head,
            merge_commit_sha: proof.merge_commit,
            checked_out_head_sha: proof.checked_out_head,
            specification_revision: specification.seq,
            specification_hash: specification.content_hash,
            revision,
            recorded_at_ms: now,
        };
        record.validate()?;
        let projection = CurrentProjection {
            schema_version: 1,
            uid: task.feature_uid.clone(),
            created_at_ms: now,
            updated_at_ms: now,
            revision,
            record: record.clone(),
        };
        let history_uid = uuid::Uuid::new_v5(
            &uuid::Uuid::parse_str(&task.feature_uid)
                .map_err(|_| StoreError::MalformedState("invalid feature UID".into()))?,
            format!("current-implementation-v1:{revision}").as_bytes(),
        )
        .hyphenated()
        .to_string();
        let history_path = format!("state/current_implementation_history/{history_uid}.json");
        let history_projection = CurrentProjection {
            uid: history_uid,
            ..projection.clone()
        };
        let bytes = |value: &CurrentProjection| {
            serde_json::to_vec_pretty(value)
                .map_err(|error| StoreError::MalformedState(error.to_string()))
        };
        before_commit();
        let mut checks = source_checks.to_vec();
        checks.extend([
            RecordRevisionCheck {
                path: current_path.clone(),
                expected_revision: current_revision,
            },
            RecordRevisionCheck {
                path: history_path.clone(),
                expected_revision: 0,
            },
        ]);
        self.store.transaction_with_record_revisions(
            &[
                (current_path.clone(), bytes(&projection)?),
                (history_path.clone(), bytes(&history_projection)?),
            ],
            &checks,
        )?;
        Ok(record)
    }

    pub fn load_current_implementation(
        &self,
        feature_uid: &str,
    ) -> Result<Option<CurrentImplementationEvidenceV1>, StoreError> {
        if !crate::core::lifecycle::types::is_uuid(feature_uid) {
            return Err(StoreError::MalformedState("feature UID is invalid".into()));
        }
        let path = format!("state/current_implementations/{feature_uid}.json");
        match self.store.read_record::<CurrentProjection>(&path) {
            Ok((projection, actual))
                if projection.uid == feature_uid
                    && projection.created_at_ms > 0
                    && projection.updated_at_ms >= projection.created_at_ms
                    && projection.revision == actual =>
            {
                projection.record.validate()?;
                Ok(Some(projection.record))
            }
            Ok(_) => Err(StoreError::MalformedState(
                "current implementation identity is inconsistent".into(),
            )),
            Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }
}
