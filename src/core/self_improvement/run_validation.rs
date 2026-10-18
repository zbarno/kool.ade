use super::validation::{is_digest, is_git_sha, is_uuid, valid_pr_url, valid_ref, valid_text};
use super::{SelfImprovementPhaseV1, SelfImprovementRunV1};

impl SelfImprovementRunV1 {
    pub fn validate(&self) -> Result<(), String> {
        let valid_optional_digest = |value: &Option<String>| value.as_deref().is_none_or(is_digest);
        if self.schema_version != 1
            || !is_uuid(&self.run_uid)
            || !is_uuid(&self.opportunity_uid)
            || !is_uuid(&self.approved_decision_uid)
            || !is_uuid(&self.target_repository_uid)
            || !is_uuid(&self.feature_uid)
            || self.task_uids.len() > 128
            || self.task_uids.iter().any(|uid| !is_uuid(uid))
            || self
                .owner_task_uid
                .as_deref()
                .is_some_and(|uid| !is_uuid(uid))
            || self
                .owner_task_uid
                .as_ref()
                .is_some_and(|uid| !self.task_uids.contains(uid))
            || self.parent_task_progress.len() > 128
            || self.parent_task_progress.iter().any(|progress| {
                !is_uuid(&progress.task_uid)
                    || !self.task_uids.contains(&progress.task_uid)
                    || progress
                        .implementation_sha256
                        .as_deref()
                        .is_some_and(|sha| !is_digest(sha))
                    || progress
                        .code_review_uid
                        .as_deref()
                        .is_some_and(|uid| !is_uuid(uid))
                    || progress
                        .runtime_qa_uid
                        .as_deref()
                        .is_some_and(|uid| !is_uuid(uid))
                    || progress
                        .spec_receipt_uid
                        .as_deref()
                        .is_some_and(|uid| !is_uuid(uid))
                    || progress
                        .pr_work_item_uid
                        .as_deref()
                        .is_some_and(|uid| !is_uuid(uid))
                    || progress
                        .pr_url
                        .as_deref()
                        .is_some_and(|url| !valid_pr_url(url))
                    || progress
                        .pr_head_sha
                        .as_deref()
                        .is_some_and(|sha| !is_git_sha(sha))
                    || progress
                        .pr_base_branch
                        .as_deref()
                        .is_some_and(|branch| !valid_ref(&format!("refs/heads/{branch}")))
                    || progress
                        .pr_merged_commit_sha
                        .as_deref()
                        .is_some_and(|sha| !is_git_sha(sha))
                    || progress
                        .pr_merged_at
                        .as_deref()
                        .is_some_and(|value| chrono::DateTime::parse_from_rfc3339(value).is_err())
            })
            || self
                .parent_task_progress
                .iter()
                .enumerate()
                .any(|(index, progress)| {
                    self.parent_task_progress[..index]
                        .iter()
                        .any(|prior| prior.task_uid == progress.task_uid)
                })
            || self
                .task_uids
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != self.task_uids.len()
            || self.canonical_remote != "https://github.com/zbarno/kool.ade"
            || self
                .source_ref
                .as_deref()
                .is_some_and(|value| !valid_ref(value))
            || self
                .destination_ref
                .as_deref()
                .is_some_and(|value| !valid_ref(value))
            || !is_git_sha(&self.source_sha)
            || self.revision == 0
            || self.clone_uid.as_deref().is_some_and(|uid| !is_uuid(uid))
            || !valid_optional_digest(&self.plan_approval_hash)
            || self
                .independent_plan_review_uid
                .as_deref()
                .is_some_and(|uid| !is_uuid(uid))
            || self
                .implementation_head_sha
                .as_deref()
                .is_some_and(|sha| !is_git_sha(sha))
            || !valid_optional_digest(&self.implementation_subject_sha256)
            || self.code_review_uids.iter().any(|uid| !is_uuid(uid))
            || self.runtime_qa_uids.iter().any(|uid| !is_uuid(uid))
            || self
                .spec_receipt_uid
                .as_deref()
                .is_some_and(|uid| !is_uuid(uid))
            || self
                .pr_work_item_uid
                .as_deref()
                .is_some_and(|uid| !is_uuid(uid))
            || self
                .merged_sha
                .as_deref()
                .is_some_and(|sha| !is_git_sha(sha))
            || self
                .activated_version
                .as_deref()
                .is_some_and(|version| !valid_text(version, 128))
            || self.pr_url.as_deref().is_some_and(|url| !valid_pr_url(url))
        {
            return Err("self-improvement run identity or provenance is invalid".into());
        }
        if self.task_uids.is_empty() != self.owner_task_uid.is_none() {
            return Err("self-improvement task ownership is incomplete".into());
        }
        let has_task_owner = !self.task_uids.is_empty() && self.owner_task_uid.is_some();
        let requirements_met = match self.phase {
            SelfImprovementPhaseV1::ApprovedToPlan | SelfImprovementPhaseV1::Planning => true,
            SelfImprovementPhaseV1::Ready => {
                self.plan_approval_hash.is_some()
                    && self.independent_plan_review_uid.is_some()
                    && self.source_ref.is_some()
                    && self.destination_ref.is_some()
                    && has_task_owner
            }
            SelfImprovementPhaseV1::Implementing => {
                self.clone_uid.is_some()
                    && self.source_ref.is_some()
                    && self.destination_ref.is_some()
                    && has_task_owner
            }
            SelfImprovementPhaseV1::Reviewing => {
                has_task_owner
                    && self.clone_uid.is_some()
                    && self.implementation_head_sha.is_some()
                    && self.implementation_subject_sha256.is_some()
            }
            SelfImprovementPhaseV1::Qa => {
                has_task_owner
                    && self.implementation_head_sha.is_some()
                    && self.implementation_subject_sha256.is_some()
                    && !self.code_review_uids.is_empty()
            }
            SelfImprovementPhaseV1::SpecSync => {
                has_task_owner
                    && self.implementation_head_sha.is_some()
                    && self.implementation_subject_sha256.is_some()
                    && !self.code_review_uids.is_empty()
                    && !self.runtime_qa_uids.is_empty()
            }
            SelfImprovementPhaseV1::DoneAwaitingPr => self.spec_receipt_uid.is_some(),
            SelfImprovementPhaseV1::PrPending => self.pr_work_item_uid.is_some(),
            SelfImprovementPhaseV1::PrOpened => {
                self.pr_work_item_uid.is_some() && self.pr_url.is_some()
            }
            SelfImprovementPhaseV1::PrClosedUnmerged => {
                self.pr_work_item_uid.is_some() && self.pr_url.is_some()
            }
            SelfImprovementPhaseV1::MergedNotActivated => {
                self.pr_url.is_some()
                    && self.merged_sha.is_some()
                    && self.activated_version.is_none()
            }
            SelfImprovementPhaseV1::Activated => {
                self.merged_sha.is_some()
                    && self
                        .activated_version
                        .as_ref()
                        .is_some_and(|value| valid_text(value, 128))
            }
            SelfImprovementPhaseV1::NeedsAttention => {
                self.plan_approval_hash.is_some()
                    && self.independent_plan_review_uid.is_some()
                    && self.source_ref.is_some()
                    && self.destination_ref.is_some()
                    && has_task_owner
                    && !self.parent_task_progress.is_empty()
            }
            SelfImprovementPhaseV1::Cancelled => true,
        };
        if !requirements_met {
            return Err("self-improvement phase is missing its required lifecycle evidence".into());
        }
        Ok(())
    }

    pub(super) fn check_revision(&self, expected: u64) -> Result<(), String> {
        self.validate()?;
        if self.revision != expected || self.revision == u64::MAX {
            return Err("self-improvement revision is stale or exhausted".into());
        }
        Ok(())
    }

    pub(super) fn bump_revision(&mut self) {
        self.revision += 1;
    }
}
