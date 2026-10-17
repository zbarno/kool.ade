use super::*;

impl QuestionStore<'_> {
    /// Save an answer or deferral with recomputed readiness so a crash cannot
    /// leave the shared question and checklist at different states.
    pub(crate) fn save_question_and_reassess(
        &self,
        question: &super::super::QuestionV1,
        question_record_revision: u64,
        expected_planning_revision: &str,
    ) -> Result<Option<ReadinessResult>, StoreError> {
        let Some((mut checklist, readiness_record_revision)) =
            self.load_readiness(&question.feature_uid)?
        else {
            self.save_with_planning_revision(
                question,
                question_record_revision,
                expected_planning_revision,
            )?;
            return Ok(None);
        };
        if checklist.plan_revision != question.plan_revision {
            return Err(StoreError::MalformedState(
                "readiness checklist belongs to a different plan revision".into(),
            ));
        }
        let mut questions = self
            .list()?
            .into_iter()
            .filter(|item| {
                item.feature_uid == question.feature_uid
                    && item.plan_revision == question.plan_revision
            })
            .collect::<Vec<_>>();
        let existing = questions
            .iter_mut()
            .find(|item| item.uid == question.uid)
            .ok_or_else(|| {
                StoreError::MalformedState("question disappeared before readiness update".into())
            })?;
        *existing = question.clone();
        let required = checklist.required_requirement_ids.clone().ok_or_else(|| {
            StoreError::MalformedState(
                "readiness checklist has no specification requirement snapshot".into(),
            )
        })?;
        let result = super::super::super::assess_readiness(&questions, &checklist, &required);
        checklist.ready = matches!(&result, ReadinessResult::Ready);
        checklist.unresolved_blocker_ids = questions
            .iter()
            .filter(|item| {
                item.priority == QuestionPriority::Blocking
                    && matches!(
                        item.status,
                        QuestionStatus::Draft | QuestionStatus::Open | QuestionStatus::Declined
                    )
                    && item.deferred.is_none()
            })
            .map(|item| item.uid.clone())
            .collect();
        checklist.revision = checklist
            .revision
            .checked_add(1)
            .ok_or_else(|| StoreError::MalformedState("readiness revision is exhausted".into()))?;
        self.save_batch_with_readiness(
            &[(question.clone(), question_record_revision)],
            &checklist,
            readiness_record_revision,
            expected_planning_revision,
        )?;
        Ok(Some(result))
    }
}
