//! Read model for the planning and task board, assembled by the application
//! before rendering so the board does not query unrelated project state.

use std::collections::BTreeSet;

#[derive(Clone, Default)]
pub struct ViewModel {
    pub task_documents: Vec<crate::artifacts::task_docs::TaskDocument>,
    pub planning_work: Vec<crate::core::planning_work::Work>,
    pub planning_items: Vec<crate::domain::item::OpenItem>,
    pub clarification_questions: Vec<crate::core::planning::QuestionV1>,
    pub clarification_error: Option<String>,
    pub opportunities: Vec<(crate::core::opportunity::OpportunityV1, u64)>,
    pub opportunity_decisions:
        std::collections::BTreeMap<String, crate::core::opportunity::OpportunityDecisionV1>,
    pub self_improvement_runs:
        std::collections::BTreeMap<String, crate::core::self_improvement::SelfImprovementRunV1>,
    pub opportunity_error: Option<String>,
    pub opportunity_discovery_settings: crate::core::opportunity::OpportunityDiscoverySettingsV1,
    /// Transient probe errors; readiness tasks live in the persisted collection.
    pub setup_attention: Vec<crate::app::setup_attention::SetupIssue>,
    pub setup_tasks: Vec<crate::persistence::setup_tasks::LocalTask>,
    pub setup_checking: bool,
    pub eligible_item_ids: BTreeSet<String>,
    /// Identities with live implementation workers or task turns.
    pub active_work_keys: BTreeSet<String>,
    pub archived: BTreeSet<String>,
    pub cancelled: BTreeSet<String>,
    /// Persisted lifecycle phase by task-document path; rendering only.
    pub lifecycle_tasks: std::collections::BTreeMap<String, crate::core::lifecycle::TaskPhase>,
    pub lifecycle_pr_statuses:
        std::collections::BTreeMap<String, crate::core::lifecycle::PullRequestPreparationStatus>,
    /// Separate PR-preparation cards linked to their completed parent tasks.
    pub lifecycle_pr_work: Vec<LifecyclePrBoardTask>,
    pub lifecycle_features: Vec<crate::core::lifecycle::FeatureGoal>,
    pub lifecycle_feature_tasks: Vec<crate::core::lifecycle::TaskRecord>,
    pub lifecycle_deployment_evidence: std::collections::BTreeMap<
        String,
        (
            crate::core::lifecycle::DeploymentEvidenceV1,
            crate::core::lifecycle::CurrentImplementationEvidenceV1,
        ),
    >,
    pub lifecycle_error: Option<String>,
}

#[derive(Clone, Default)]
pub struct BootstrapConversationView {
    pub messages: Vec<crate::domain::chatlog::ChatMessage>,
    pub busy: bool,
    pub active_for_task: bool,
    pub error: Option<String>,
    pub suggestion: Option<BootstrapSuggestionView>,
}

#[derive(Clone)]
pub struct BootstrapSuggestionView {
    pub operation: BootstrapSuggestionOperation,
    pub turn_generation: u64,
}

#[derive(Clone)]
pub enum BootstrapSuggestionOperation {
    RecheckPrerequisites,
    PrepareLockedImage { profile: String },
    OpenCodingTools,
}

#[derive(Clone)]
pub struct LifecyclePrBoardTask {
    pub parent_path: String,
    pub parent_title: String,
    pub preparation: crate::core::lifecycle::PullRequestPreparation,
}

impl ViewModel {
    pub fn is_archived(&self, key: &str) -> bool {
        self.archived.contains(key)
    }

    pub fn is_task_intent_archived(&self, uid: &str) -> bool {
        self.planning_work
            .iter()
            .find(|work| work.uid == uid)
            .is_some_and(|work| {
                self.can_archive_task_intent(work)
                    && (work.task_intent_archived_at_ms.is_some() || self.is_archived(&work.key))
            })
    }

    pub fn task_intent_lifecycle(
        &self,
        root: &crate::core::planning_work::Work,
    ) -> crate::core::task_intent::TaskIntentLifecycle {
        let current_cycle = root.task_intent_cycles.last();
        let cycle_uid = current_cycle.map(|cycle| cycle.uid.as_str());
        let cycle_work = current_cycle.and_then(|cycle| {
            self.planning_work
                .iter()
                .find(|candidate| candidate.uid == cycle.work_uid)
        });
        let mut aggregate_root = root.clone();
        if let Some(cycle_work) = cycle_work
            && cycle_work.uid != root.uid
        {
            aggregate_root.status = cycle_work.status;
            aggregate_root.feature_id = cycle_work.feature_id.clone();
            aggregate_root.feature_uid = cycle_work.feature_uid.clone();
        }
        let mut children = self
            .planning_work
            .iter()
            .filter(|work| {
                work.uid != root.uid && work.task_intent_uid.as_deref() == Some(root.uid.as_str())
            })
            .filter(|work| {
                cycle_uid.is_none_or(|uid| work.task_intent_cycle_uid.as_deref() == Some(uid))
            })
            .map(|work| work.status)
            .collect::<Vec<_>>();
        let mut task_phases = Vec::new();
        for document in &self.task_documents {
            if crate::artifacts::task_docs::task_intent_uid_for_document(
                document,
                &self.task_documents,
                &self.planning_work,
            )
            .as_deref()
                != Some(root.uid.as_str())
                || cycle_uid.is_some_and(|uid| {
                    crate::artifacts::task_docs::task_intent_cycle_uid_for_document(
                        document,
                        &self.task_documents,
                        &self.planning_work,
                    )
                    .as_deref()
                        != Some(uid)
                })
            {
                continue;
            }
            if let Some(phase) = self.lifecycle_tasks.get(&document.path) {
                task_phases.push(*phase);
            }
            if let Some(state) = &document.task_state {
                use crate::artifacts::task_docs::ExecutionPublicationStatus as Publication;
                let phase = match state.publication_status {
                    Publication::Verified => Some(crate::core::lifecycle::TaskPhase::Done),
                    Publication::Shared => Some(crate::core::lifecycle::TaskPhase::PrOpened),
                    Publication::Merged => Some(crate::core::lifecycle::TaskPhase::PrMerged),
                    Publication::ClosedUnmerged | Publication::Failed => {
                        Some(crate::core::lifecycle::TaskPhase::Failed)
                    }
                    Publication::Planned | Publication::Running => match state.status {
                        crate::core::planning_work::WorkStatus::Todo => {
                            Some(crate::core::lifecycle::TaskPhase::Ready)
                        }
                        crate::core::planning_work::WorkStatus::InProgress => {
                            Some(crate::core::lifecycle::TaskPhase::Running)
                        }
                        crate::core::planning_work::WorkStatus::InReview => {
                            Some(crate::core::lifecycle::TaskPhase::AwaitingReview)
                        }
                        crate::core::planning_work::WorkStatus::NeedsAttention => {
                            Some(crate::core::lifecycle::TaskPhase::Blocked)
                        }
                        crate::core::planning_work::WorkStatus::Done => {
                            Some(crate::core::lifecycle::TaskPhase::Done)
                        }
                    },
                };
                if let Some(phase) = phase {
                    task_phases.push(phase);
                    children.push(state.status);
                }
            }
        }
        let decisions = self
            .planning_items
            .iter()
            .filter(|item| item.task_intent_uid.as_deref() == Some(root.uid.as_str()))
            .filter(|item| {
                cycle_uid.is_none_or(|uid| {
                    item.task_intent_cycle_uid.as_deref() == Some(uid)
                        || (root.task_intent_cycles.len() == 1
                            && item.task_intent_cycle_uid.is_none())
                })
            })
            .map(|item| item.status)
            .collect::<Vec<_>>();
        let feature = aggregate_root.feature_uid.as_deref().and_then(|uid| {
            let mut matches = self
                .lifecycle_features
                .iter()
                .filter(|feature| feature.feature_uid == uid);
            let feature = matches.next()?;
            matches.next().is_none().then_some(feature)
        });
        task_phases.extend(
            feature
                .into_iter()
                .flat_map(|feature| {
                    self.lifecycle_feature_tasks
                        .iter()
                        .filter(move |task| task.feature_uid == feature.feature_uid)
                })
                .map(|task| task.phase),
        );
        let deployment = feature.and_then(|feature| {
            self.lifecycle_deployment_evidence
                .get(&feature.feature_uid)
                .map(|(deployment, current)| (deployment, current))
        });
        let cancelled = self
            .cancelled
            .contains(&crate::persistence::cancelled_work::planning_id(&root.uid));
        crate::core::task_intent::TaskIntent::project_with_decisions(
            &aggregate_root,
            &children,
            &decisions,
            feature.map(|feature| feature.phase),
            &task_phases,
            deployment,
            cancelled,
        )
        .map_or(
            crate::core::task_intent::TaskIntentLifecycle::Unknown,
            |intent| intent.lifecycle,
        )
    }

    pub fn can_archive_task_intent(&self, root: &crate::core::planning_work::Work) -> bool {
        matches!(
            self.task_intent_lifecycle(root),
            crate::core::task_intent::TaskIntentLifecycle::Complete
                | crate::core::task_intent::TaskIntentLifecycle::Verified
                | crate::core::task_intent::TaskIntentLifecycle::Merged
                | crate::core::task_intent::TaskIntentLifecycle::Deployed
                | crate::core::task_intent::TaskIntentLifecycle::Cancelled
                | crate::core::task_intent::TaskIntentLifecycle::Failed
        ) && !self.has_active_task_intent_work(root)
    }

    fn has_active_task_intent_work(&self, root: &crate::core::planning_work::Work) -> bool {
        let owns_identity = |identity: &str| self.active_work_keys.contains(identity);
        self.planning_work
            .iter()
            .filter(|work| {
                work.uid == root.uid || work.task_intent_uid.as_deref() == Some(root.uid.as_str())
            })
            .any(|work| owns_identity(&work.uid) || owns_identity(&work.key))
            || self.task_documents.iter().any(|document| {
                crate::artifacts::task_docs::task_intent_uid_for_document(
                    document,
                    &self.task_documents,
                    &self.planning_work,
                )
                .as_deref()
                    == Some(root.uid.as_str())
                    && (owns_identity(&document.path)
                        || document
                            .metadata
                            .as_ref()
                            .is_some_and(|metadata| owns_identity(&metadata.uid)))
            })
    }
}
