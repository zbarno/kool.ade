use super::validation::{is_digest, is_git_sha, is_uuid, valid_pr_url, valid_text};
use super::{
    SelfImprovementEventV1, SelfImprovementParentTaskProgressV1, SelfImprovementPhaseV1,
    SelfImprovementRunV1,
};

impl SelfImprovementRunV1 {
    pub fn apply(
        &mut self,
        expected_revision: u64,
        event: SelfImprovementEventV1,
    ) -> Result<(), String> {
        self.check_revision(expected_revision)?;
        match event {
            SelfImprovementEventV1::BeginPlanning
                if self.phase == SelfImprovementPhaseV1::ApprovedToPlan =>
            {
                self.phase = SelfImprovementPhaseV1::Planning;
            }
            SelfImprovementEventV1::PlanApproved {
                approval_sha256,
                capability_id,
                review_uid,
                source_ref,
                destination_ref,
                task_uids,
                owner_task_uid,
                parent_task_uids,
            } if self.phase == SelfImprovementPhaseV1::Planning
                && is_digest(&approval_sha256)
                && capability_id
                    .as_deref()
                    .is_some_and(super::capability_probe::valid_capability_id)
                && is_uuid(&review_uid)
                && super::validation::valid_ref(&source_ref)
                && super::validation::valid_ref(&destination_ref)
                && !task_uids.is_empty()
                && task_uids.len() <= 128
                && task_uids.iter().all(|uid| is_uuid(uid))
                && task_uids
                    .iter()
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
                    == task_uids.len()
                && is_uuid(&owner_task_uid)
                && task_uids.contains(&owner_task_uid)
                && {
                    let parents = if parent_task_uids.is_empty() {
                        vec![owner_task_uid.clone()]
                    } else {
                        parent_task_uids.clone()
                    };
                    parents
                        .iter()
                        .all(|uid| is_uuid(uid) && task_uids.contains(uid))
                        && parents
                            .iter()
                            .collect::<std::collections::BTreeSet<_>>()
                            .len()
                            == parents.len()
                } =>
            {
                let parent_task_uids = if parent_task_uids.is_empty() {
                    vec![owner_task_uid.clone()]
                } else {
                    parent_task_uids
                };
                self.parent_task_progress = parent_task_uids
                    .into_iter()
                    .map(|task_uid| SelfImprovementParentTaskProgressV1 {
                        task_uid,
                        phase: crate::core::lifecycle::TaskPhase::Ready,
                        implementation_sha256: None,
                        code_review_uid: None,
                        runtime_qa_uid: None,
                        spec_receipt_uid: None,
                        pr_work_item_uid: None,
                        pr_status: None,
                        pr_url: None,
                        pr_head_sha: None,
                        pr_base_branch: None,
                        pr_merged_commit_sha: None,
                        pr_merged_at: None,
                    })
                    .collect();
                self.plan_approval_hash = Some(approval_sha256);
                self.capability_id = capability_id;
                self.independent_plan_review_uid = Some(review_uid);
                self.source_ref = Some(source_ref);
                self.destination_ref = Some(destination_ref);
                self.task_uids = task_uids;
                self.owner_task_uid = Some(owner_task_uid);
                self.phase = SelfImprovementPhaseV1::Ready;
            }
            SelfImprovementEventV1::ParentTaskProgress(progress)
                if self.phase != SelfImprovementPhaseV1::Cancelled
                    && self.phase != SelfImprovementPhaseV1::ApprovedToPlan
                    && self.phase != SelfImprovementPhaseV1::Planning
                    && self
                        .parent_task_progress
                        .iter()
                        .any(|item| item.task_uid == progress.task_uid) =>
            {
                let current = self
                    .parent_task_progress
                    .iter_mut()
                    .find(|item| item.task_uid == progress.task_uid)
                    .expect("parent progress was checked above");
                *current = progress;
            }
            SelfImprovementEventV1::CloneAllocated { clone_uid }
                if self.phase == SelfImprovementPhaseV1::Ready && is_uuid(&clone_uid) =>
            {
                self.clone_uid = Some(clone_uid);
                self.phase = SelfImprovementPhaseV1::Implementing;
            }
            SelfImprovementEventV1::ImplementationFinished {
                head_sha,
                subject_sha256,
            } if (self.phase == SelfImprovementPhaseV1::Implementing
                || matches!(
                    self.phase,
                    SelfImprovementPhaseV1::Reviewing
                        | SelfImprovementPhaseV1::Qa
                        | SelfImprovementPhaseV1::SpecSync
                        | SelfImprovementPhaseV1::DoneAwaitingPr
                ) && self.implementation_subject_sha256.as_deref()
                    != Some(subject_sha256.as_str()))
                && is_git_sha(&head_sha)
                && is_digest(&subject_sha256) =>
            {
                self.implementation_head_sha = Some(head_sha);
                self.implementation_subject_sha256 = Some(subject_sha256);
                self.code_review_uids.clear();
                self.runtime_qa_uids.clear();
                self.spec_receipt_uid = None;
                self.pr_work_item_uid = None;
                self.pr_url = None;
                self.merged_sha = None;
                self.activated_version = None;
                self.phase = SelfImprovementPhaseV1::Reviewing;
            }
            SelfImprovementEventV1::CodeReviewPassed {
                receipt_uid,
                subject_sha256,
            } if self.phase == SelfImprovementPhaseV1::Reviewing
                && is_uuid(&receipt_uid)
                && self.implementation_subject_sha256.as_deref()
                    == Some(subject_sha256.as_str()) =>
            {
                self.code_review_uids.push(receipt_uid);
                self.phase = SelfImprovementPhaseV1::Qa;
            }
            SelfImprovementEventV1::QaPassed {
                receipt_uid,
                subject_sha256,
            } if self.phase == SelfImprovementPhaseV1::Qa
                && is_uuid(&receipt_uid)
                && self.implementation_subject_sha256.as_deref()
                    == Some(subject_sha256.as_str()) =>
            {
                self.runtime_qa_uids.push(receipt_uid);
                self.phase = SelfImprovementPhaseV1::SpecSync;
            }
            SelfImprovementEventV1::SpecReconciled {
                receipt_uid,
                subject_sha256,
            } if self.phase == SelfImprovementPhaseV1::SpecSync
                && is_uuid(&receipt_uid)
                && self.implementation_subject_sha256.as_deref()
                    == Some(subject_sha256.as_str()) =>
            {
                self.spec_receipt_uid = Some(receipt_uid);
                self.phase = SelfImprovementPhaseV1::DoneAwaitingPr;
            }
            SelfImprovementEventV1::PrPrepared { work_item_uid }
                if self.phase == SelfImprovementPhaseV1::DoneAwaitingPr
                    && is_uuid(&work_item_uid) =>
            {
                self.pr_work_item_uid = Some(work_item_uid);
                self.phase = SelfImprovementPhaseV1::PrPending;
            }
            SelfImprovementEventV1::PrOpened { url }
                if self.phase == SelfImprovementPhaseV1::PrPending && valid_pr_url(&url) =>
            {
                self.pr_url = Some(url);
                self.phase = SelfImprovementPhaseV1::PrOpened;
            }
            SelfImprovementEventV1::PrClosedUnmerged { url }
                if self.phase == SelfImprovementPhaseV1::PrOpened && valid_pr_url(&url) =>
            {
                self.pr_url = Some(url);
                self.phase = SelfImprovementPhaseV1::PrClosedUnmerged;
            }
            SelfImprovementEventV1::PrReopened
                if self.phase == SelfImprovementPhaseV1::PrClosedUnmerged
                    && self.pr_url.as_deref().is_some_and(valid_pr_url) =>
            {
                self.phase = SelfImprovementPhaseV1::PrOpened;
            }
            SelfImprovementEventV1::Merged { sha }
                if self.phase == SelfImprovementPhaseV1::PrOpened && is_git_sha(&sha) =>
            {
                self.merged_sha = Some(sha);
                self.phase = SelfImprovementPhaseV1::MergedNotActivated;
            }
            SelfImprovementEventV1::Installed {
                version,
                build_commit_sha,
                operator_id,
            } if self.schema_version >= 2
                && self.phase == SelfImprovementPhaseV1::MergedNotActivated
                && valid_text(&version, 128)
                && valid_text(&operator_id, 128)
                && Some(build_commit_sha.as_str()) == self.merged_sha.as_deref()
                && is_git_sha(&build_commit_sha) =>
            {
                self.installed_version = Some(version);
                self.installed_build_commit_sha = Some(build_commit_sha);
                self.phase = SelfImprovementPhaseV1::InstalledNotActivated;
            }
            SelfImprovementEventV1::Activated { evidence }
                if self.phase == SelfImprovementPhaseV1::InstalledNotActivated
                    && evidence.feature_uid == self.feature_uid
                    && Some(evidence.merged_sha.as_str()) == self.merged_sha.as_deref()
                    && Some(evidence.version.as_str()) == self.installed_version.as_deref()
                    && Some(evidence.build_commit_sha.as_str())
                        == self.installed_build_commit_sha.as_deref()
                    && evidence.build_commit_sha == evidence.merged_sha
                    && is_git_sha(&evidence.build_commit_sha)
                    && is_digest(&evidence.capability_manifest_sha256)
                    && evidence.capability_id == "self-improvement-lifecycle"
                    && self.capability_id.as_deref() == Some(evidence.capability_id.as_str())
                    && evidence.readiness_probe_id == "self-improvement-lifecycle-v1"
                    && is_uuid(&evidence.readiness_probe_uid)
                    && valid_text(&evidence.version, 128) =>
            {
                self.activated_version = Some(evidence.version);
                self.phase = SelfImprovementPhaseV1::Activated;
            }
            SelfImprovementEventV1::NeedsAttention
                if matches!(
                    self.phase,
                    SelfImprovementPhaseV1::Ready
                        | SelfImprovementPhaseV1::Implementing
                        | SelfImprovementPhaseV1::Reviewing
                        | SelfImprovementPhaseV1::Qa
                        | SelfImprovementPhaseV1::SpecSync
                ) =>
            {
                self.phase = SelfImprovementPhaseV1::NeedsAttention;
            }
            SelfImprovementEventV1::Cancel
                if !matches!(
                    self.phase,
                    SelfImprovementPhaseV1::PrOpened
                        | SelfImprovementPhaseV1::PrClosedUnmerged
                        | SelfImprovementPhaseV1::MergedNotActivated
                        | SelfImprovementPhaseV1::InstalledNotActivated
                        | SelfImprovementPhaseV1::Activated
                ) =>
            {
                self.phase = SelfImprovementPhaseV1::Cancelled;
            }
            _ => {
                return Err(
                    "self-improvement event is invalid for the current phase or evidence".into(),
                );
            }
        }
        self.bump_revision();
        self.validate()
    }
}
