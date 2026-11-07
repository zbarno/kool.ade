use super::ViewModel;
use crate::{
    core::{planning::QuestionStatus, planning_work::Work},
    ui::feature_approval::Action,
};

pub(crate) fn question_column(status: QuestionStatus) -> usize {
    match status {
        QuestionStatus::Draft
        | QuestionStatus::Open
        | QuestionStatus::Declined
        | QuestionStatus::Deferred => 3,
        QuestionStatus::Answered
        | QuestionStatus::EvidenceResolved
        | QuestionStatus::Superseded
        | QuestionStatus::Cancelled => 4,
    }
}

impl ViewModel {
    /// Ownership, rather than an old per-card archive flag, governs visibility.
    pub(crate) fn is_visible(&self, key: &str, owner: Option<&str>) -> bool {
        owner.map_or_else(
            || !self.is_archived(key),
            |uid| !self.is_task_intent_archived(uid),
        )
    }

    pub(crate) fn intent_for_feature(&self, uid: &str) -> Option<&Work> {
        let mut roots = self
            .planning_work
            .iter()
            .filter(|work| crate::ui::feature_approval::belongs_to_root(work, uid));
        let root = roots.next()?;
        roots.next().is_none().then_some(root)
    }

    pub(crate) fn feature_visible(&self, uid: &str) -> bool {
        self.intent_for_feature(uid)
            .is_none_or(|root| !self.is_task_intent_archived(&root.uid))
    }

    /// Historical questions retain the request owner across follow-up cycles.
    pub(crate) fn question_owner(&self, feature_uid: &str) -> Option<&Work> {
        let mut owners = self.planning_work.iter().filter(|root| {
            root.kind.starts_task_intent()
                && root.parent_uid.is_none()
                && root.task_intent_root_uid() == Some(root.uid.as_str())
                && self.planning_work.iter().any(|work| {
                    work.feature_uid.as_deref() == Some(feature_uid)
                        && work.task_intent_root_uid() == Some(root.uid.as_str())
                })
        });
        let root = owners.next()?;
        owners.next().is_none().then_some(root)
    }

    pub(crate) fn question_visible(&self, question: &crate::core::planning::QuestionV1) -> bool {
        self.question_owner(&question.feature_uid)
            .is_none_or(|root| !self.is_task_intent_archived(&root.uid))
    }

    pub(crate) fn visible_card_count(&self, actions: &[Action]) -> usize {
        let work = self
            .planning_work
            .iter()
            .filter(|work| self.is_visible(&work.key, work.task_intent_root_uid()))
            .count();
        let tasks = self
            .task_documents
            .iter()
            .filter(|task| {
                !task.path.ends_with("/README.md")
                    && self.is_visible(
                        &task.path,
                        crate::artifacts::task_docs::task_intent_uid_for_document(
                            task,
                            &self.task_documents,
                            &self.planning_work,
                        )
                        .as_deref(),
                    )
            })
            .count();
        let items = self
            .planning_items
            .iter()
            .filter(|item| {
                self.is_visible(item.conversation_key(), item.task_intent_uid.as_deref())
            })
            .count();
        let pr = self
            .lifecycle_pr_work
            .iter()
            .filter(|pr| {
                self.task_documents
                    .iter()
                    .find(|task| task.path == pr.parent_path)
                    .is_none_or(|task| {
                        self.is_visible(
                            &pr.preparation.uid,
                            crate::artifacts::task_docs::task_intent_uid_for_document(
                                task,
                                &self.task_documents,
                                &self.planning_work,
                            )
                            .as_deref(),
                        )
                    })
            })
            .count();
        work + tasks
            + items
            + pr
            + self
                .clarification_questions
                .iter()
                .filter(|question| self.question_visible(question))
                .count()
            + self
                .lifecycle_features
                .iter()
                .filter(|feature| {
                    feature.phase == crate::core::lifecycle::FeaturePhase::PlanReviewPending
                        && self.feature_visible(&feature.feature_uid)
                })
                .count()
            + actions
                .iter()
                .filter(|action| {
                    !action.approved
                        && action
                            .plan_snapshot
                            .as_ref()
                            .is_none_or(|snapshot| self.feature_visible(&snapshot.feature_uid))
                })
                .count()
            + self
                .setup_tasks
                .iter()
                .filter(|task| task.is_active())
                .count()
            + self.setup_attention.len()
    }
}

#[cfg(test)]
mod tests;
