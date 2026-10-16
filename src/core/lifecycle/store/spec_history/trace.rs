use crate::core::lifecycle::{
    ProductSpecAggregateStatusV1, ProductSpecAggregateV1, ProductSpecFeatureSummaryV1,
    ProductSpecFindingV1, SpecRequirementTraceV1,
};

use super::super::LifecycleStore;

impl LifecycleStore<'_> {
    /// Resolve only decisions whose persisted source explicitly names this
    /// requirement; feature-wide plan receipts are not copied to every delta.
    pub fn requirement_decision_refs(
        &self,
        feature_uid: &str,
        requirement_id: &str,
        task_uids: &[String],
    ) -> Result<Vec<String>, crate::artifacts::planning_store::StoreError> {
        let questions = crate::core::planning::QuestionStore::new(self.store).list()?;
        let mut references = questions
            .into_iter()
            .filter(|question| {
                question.feature_uid == feature_uid
                    && question
                        .origin_requirement_ids
                        .iter()
                        .any(|id| id == requirement_id)
                    && matches!(
                        question.status,
                        crate::core::planning::QuestionStatus::Answered
                            | crate::core::planning::QuestionStatus::Declined
                    )
            })
            .map(|question| question.uid)
            .collect::<Vec<_>>();
        for task_uid in task_uids {
            let task = self.load_task(task_uid)?;
            if task.feature_uid != feature_uid
                || !task
                    .approved_requirement_ids
                    .iter()
                    .any(|id| id == requirement_id)
            {
                continue;
            }
            references.extend(
                task.code_review
                    .iter()
                    .chain(task.runtime_qa.iter())
                    .chain(task.review_history.iter())
                    .map(|receipt| receipt.receipt_uid.clone()),
            );
        }
        references.sort();
        references.dedup();
        Ok(references)
    }

    /// Deterministically reconcile the rendered product document against every
    /// lifecycle-backed feature receipt and its latest unresolved findings.
    pub fn product_specification_aggregate(
        &self,
    ) -> Result<ProductSpecAggregateV1, crate::artifacts::planning_store::StoreError> {
        let rendered_product_hash = crate::core::lifecycle::product_spec::current_product_hash(
            self.store,
        )
        .map_err(|error| {
            crate::artifacts::planning_store::StoreError::MalformedState(error.to_string())
        })?;
        let mut lifecycle_features = Vec::new();
        let mut findings = Vec::new();
        let mut pending_trigger_count = 0;
        let paths = self.store.list_record_paths("state/lifecycle_features")?;
        for path in paths {
            let Some(feature_uid) = path
                .rsplit('/')
                .next()
                .and_then(|name| name.strip_suffix(".json"))
            else {
                continue;
            };
            let feature = self.load_feature(feature_uid)?;
            let revision = self.load_spec_revision(feature_uid)?;
            let history = self.spec_revision_history(feature_uid)?;
            let drift_detected = revision
                .as_ref()
                .is_some_and(|revision| revision.content_hash != rendered_product_hash);
            lifecycle_features.push(ProductSpecFeatureSummaryV1 {
                feature_uid: feature_uid.to_owned(),
                revision: revision.as_ref().map(|revision| revision.seq),
                approved_target_hash: feature.approved_plan_sha256,
                reconciliation_status: revision
                    .as_ref()
                    .map(|revision| revision.reconciliation_status),
                drift_detected,
            });
            pending_trigger_count += self.list_spec_sync_queue(feature_uid, false)?.len();

            let mut unresolved = Vec::new();
            for (entry, deltas) in history {
                for delta in deltas {
                    if delta.operator_decision.is_some() {
                        unresolved.retain(
                            |(_, existing): &(
                                crate::core::lifecycle::SpecRevisionV1,
                                crate::core::lifecycle::SpecDeltaRecordV1,
                            )| {
                                existing.requirement_id != delta.requirement_id
                                    || existing.event_id != delta.event_id
                            },
                        );
                    } else {
                        unresolved.retain(
                            |(_, existing): &(
                                crate::core::lifecycle::SpecRevisionV1,
                                crate::core::lifecycle::SpecDeltaRecordV1,
                            )| {
                                existing.requirement_id != delta.requirement_id
                                    || existing.event_id != delta.event_id
                            },
                        );
                        unresolved.push((entry.clone(), delta));
                    }
                }
            }
            findings.extend(unresolved.into_iter().map(|(entry, delta)| {
                let mut decision_refs = delta.decision_refs.clone();
                if let Some(decision) = &delta.operator_decision {
                    decision_refs.push(decision.receipt_uid.clone());
                }
                decision_refs.sort();
                decision_refs.dedup();
                ProductSpecFindingV1 {
                    feature_uid: feature_uid.to_owned(),
                    revision: entry.seq,
                    requirement_id: delta.requirement_id,
                    event_id: delta.event_id,
                    decision_refs,
                    classification: delta.classification,
                    evidence_refs: delta.evidence_refs,
                    test_refs: delta.test_refs,
                    affected_task_ids: delta.affected_task_ids,
                    needs_user_reapproval: delta.needs_user_reapproval,
                }
            }));
        }
        lifecycle_features.sort_by(|left, right| left.feature_uid.cmp(&right.feature_uid));
        findings.sort_by(|left, right| {
            (
                &left.feature_uid,
                &left.requirement_id,
                left.revision,
                &left.event_id,
            )
                .cmp(&(
                    &right.feature_uid,
                    &right.requirement_id,
                    right.revision,
                    &right.event_id,
                ))
        });
        let status = if lifecycle_features
            .iter()
            .any(|feature| feature.revision.is_none())
        {
            ProductSpecAggregateStatusV1::BaselineIncomplete
        } else if lifecycle_features
            .iter()
            .any(|feature| feature.drift_detected)
        {
            ProductSpecAggregateStatusV1::DriftDetected
        } else if pending_trigger_count > 0 {
            ProductSpecAggregateStatusV1::PendingTriggers
        } else if !findings.is_empty() {
            ProductSpecAggregateStatusV1::FindingsRequireReview
        } else {
            ProductSpecAggregateStatusV1::Synchronized
        };
        Ok(ProductSpecAggregateV1 {
            rendered_product_hash,
            status,
            lifecycle_features,
            findings,
            pending_trigger_count,
        })
    }

    /// Return the complete typed forward trace for a stable feature requirement.
    pub fn specification_requirement_trace(
        &self,
        feature_uid: &str,
        requirement_id: &str,
    ) -> Result<Vec<SpecRequirementTraceV1>, crate::artifacts::planning_store::StoreError> {
        let mut traces = Vec::new();
        for (revision, deltas) in self.spec_revision_history(feature_uid)? {
            for delta in deltas
                .into_iter()
                .filter(|delta| delta.requirement_id == requirement_id)
            {
                let mut task_refs = delta.affected_task_ids;
                task_refs.extend(delta.followup_ids);
                task_refs.sort();
                task_refs.dedup();
                let mut decision_refs = delta.decision_refs.clone();
                if let Some(decision) = &delta.operator_decision {
                    decision_refs.push(decision.receipt_uid.clone());
                }
                decision_refs.sort();
                decision_refs.dedup();
                traces.push(SpecRequirementTraceV1 {
                    feature_uid: feature_uid.to_owned(),
                    revision: revision.seq,
                    requirement_id: delta.requirement_id,
                    event_id: delta.event_id,
                    decision_refs,
                    task_refs,
                    test_refs: delta.test_refs,
                    pull_request_refs: delta
                        .evidence_refs
                        .iter()
                        .filter(|reference| reference.starts_with("provider-pr-state:"))
                        .cloned()
                        .collect(),
                    evidence_refs: delta.evidence_refs,
                });
            }
        }
        Ok(traces)
    }

    /// Reverse lookup from any durable event, decision, task, test, review,
    /// QA, PR, or evidence reference to the affected stable requirements.
    pub fn requirements_for_specification_reference(
        &self,
        feature_uid: &str,
        reference: &str,
    ) -> Result<Vec<SpecRequirementTraceV1>, crate::artifacts::planning_store::StoreError> {
        let mut traces = Vec::new();
        for (revision, deltas) in self.spec_revision_history(feature_uid)? {
            for delta in deltas {
                let matches = delta.event_id == reference
                    || delta.test_refs.iter().any(|item| item == reference)
                    || delta.affected_task_ids.iter().any(|item| item == reference)
                    || delta.followup_ids.iter().any(|item| item == reference)
                    || delta.evidence_refs.iter().any(|item| item == reference)
                    || delta.decision_refs.iter().any(|item| item == reference)
                    || delta
                        .operator_decision
                        .as_ref()
                        .is_some_and(|decision| decision.receipt_uid == reference);
                if matches {
                    let mut task_refs = delta.affected_task_ids.clone();
                    task_refs.extend(delta.followup_ids.clone());
                    task_refs.sort();
                    task_refs.dedup();
                    let mut decision_refs = delta.decision_refs.clone();
                    if let Some(decision) = &delta.operator_decision {
                        decision_refs.push(decision.receipt_uid.clone());
                    }
                    decision_refs.sort();
                    decision_refs.dedup();
                    traces.push(SpecRequirementTraceV1 {
                        feature_uid: feature_uid.to_owned(),
                        revision: revision.seq,
                        requirement_id: delta.requirement_id,
                        event_id: delta.event_id,
                        decision_refs,
                        task_refs,
                        test_refs: delta.test_refs,
                        pull_request_refs: delta
                            .evidence_refs
                            .iter()
                            .filter(|item| item.starts_with("provider-pr-state:"))
                            .cloned()
                            .collect(),
                        evidence_refs: delta.evidence_refs,
                    });
                }
            }
        }
        traces.sort_by(|left, right| {
            (&left.requirement_id, left.revision).cmp(&(&right.requirement_id, right.revision))
        });
        traces.dedup_by(|left, right| {
            left.requirement_id == right.requirement_id && left.revision == right.revision
        });
        Ok(traces)
    }
}
