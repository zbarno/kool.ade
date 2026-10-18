#[path = "plan_review/lifecycle.rs"]
pub(crate) mod lifecycle;

pub(super) fn advance_plan_review(project: &mut crate::app::session::Project) {
    let result = project
        .activity
        .plan_review
        .as_ref()
        .and_then(crate::app::root::plan_review::PlanReview::take_result);
    let Some(result) = result else { return };
    let reviewed_feature = project.activity.plan_review_feature.clone();
    project.activity.plan_review = None;
    project.activity.plan_review_feature = None;
    project.activity.plan_review_revision = None;
    project.live_progress = Default::default();
    let task_gate;
    let message = match result {
        Err(error) => {
            if let Some(feature_id) = reviewed_feature.as_deref() {
                lifecycle::mark_arbitration_failed(project, feature_id, &error);
            }
            task_gate = (
                crate::core::planning_work::WorkStatus::NeedsAttention,
                format!(
                    "Blocked: independent plan review did not complete ({error}). Retry review before task generation."
                ),
            );
            format!(
                "Independent plan review did not complete: {error}. Task generation remains blocked."
            )
        }
        Ok(outcome) => {
            let review_error = outcome.review_error.is_some();
            let review_unable = matches!(
                outcome.verdict.outcome,
                Some(crate::harness::prompt_catalog::ReviewOutcomeV1::ReviewUnable)
            );
            let approved = outcome.verdict.pass && outcome.verdict.findings.is_empty();
            let decision = if approved {
                crate::core::lifecycle::GateDecision::Approved
            } else {
                crate::core::lifecycle::GateDecision::ChangesRequested
            };
            let receipt = crate::core::lifecycle::GateReceipt {
                receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
                subject_uid: outcome.snapshot_uid.clone(),
                subject_sha256: outcome.snapshot_hash.clone(),
                actor: crate::core::lifecycle::GateActor::IndependentReviewer,
                decision,
                recorded_at_unix_ms: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
                    .min(u64::MAX as u128) as u64,
                evidence_refs: outcome.evidence_refs.clone(),
                prompt_sha256: Some(outcome.prompt.identity_sha256()),
                prompt_manifest: Some(outcome.prompt),
                reviewer_run_id: Some(outcome.reviewer_run_id),
                author_run_id: Some(outcome.author_run_id),
                reviewer_session_id: Some(outcome.reviewer_session_id),
                author_session_id: Some(outcome.author_session_id),
            };
            let Some(expected_revision) = project.activity.plan_review_revision.take() else {
                return;
            };
            let arbitration_failed =
                outcome.arbitration.is_some() && (review_error || review_unable);
            let persisted = if arbitration_failed {
                Err(anyhow::anyhow!(
                    "independent arbitration did not produce a usable verdict"
                ))
            } else if let Some(arbitration) =
                outcome.arbitration.as_ref().filter(|_| !arbitration_failed)
            {
                crate::core::workflow::record_plan_arbitration_if_current(
                    &project.state.planning_store,
                    &outcome.feature_id,
                    &arbitration.finding_id,
                    &arbitration.evidence_id,
                    &arbitration.evidence_sha256,
                    &arbitration.source_content,
                    &arbitration.source_quote,
                    &arbitration.rationale,
                    &receipt,
                    &outcome.verdict,
                    &expected_revision,
                )
            } else {
                crate::core::workflow::record_plan_review_if_current(
                    &project.state.planning_store,
                    &outcome.feature_id,
                    &receipt,
                    &outcome.verdict,
                    &expected_revision,
                )
            };
            match persisted {
                Ok(revision) => {
                    project.state.baseline_planning_revision = revision;
                    let arbitration_status = outcome
                        .arbitration
                        .as_ref()
                        .filter(|_| !arbitration_failed)
                        .map(|arbitration| {
                            lifecycle::record_plan_arbitration_work(
                                project,
                                &outcome.feature_id,
                                arbitration,
                                &outcome.verdict,
                                &receipt,
                            )
                        });
                    let lifecycle_result = if approved && !arbitration_failed {
                        lifecycle::record_reviewed_plan(project, &outcome.feature_id)
                    } else {
                        Ok(())
                    };
                    let lifecycle_error = lifecycle_result.err();
                    let arbitration_error = arbitration_status.and_then(Result::err);
                    let verification_error = if approved && lifecycle_error.is_none() {
                        lifecycle::verify_manager_plan_repairs(
                            project,
                            &outcome.feature_id,
                            &outcome.snapshot_hash,
                            &receipt.receipt_uid,
                        )
                        .err()
                    } else {
                        None
                    };
                    task_gate = if approved
                        && lifecycle_error.is_none()
                        && verification_error.is_none()
                        && arbitration_error.is_none()
                    {
                        (
                            crate::core::planning_work::WorkStatus::Todo,
                            "Approved: the independent PlanReviewer approved this exact plan snapshot. Task generation is ready when requested.".into(),
                        )
                    } else if approved && arbitration_error.is_some() {
                        (
                            crate::core::planning_work::WorkStatus::NeedsAttention,
                            format!(
                                "Blocked: arbitration receipt was saved but its board status could not be synchronized ({}).",
                                arbitration_error.as_deref().unwrap_or("unknown error")
                            ),
                        )
                    } else if approved && lifecycle_error.is_some() {
                        (
                            crate::core::planning_work::WorkStatus::NeedsAttention,
                            format!(
                                "Blocked: lifecycle approval record could not be synchronized ({})",
                                lifecycle_error.as_deref().unwrap_or("unknown error")
                            ),
                        )
                    } else if approved {
                        (
                            crate::core::planning_work::WorkStatus::NeedsAttention,
                            format!(
                                "Blocked: Manager repair verification could not be saved ({}).",
                                verification_error.as_deref().unwrap_or("unknown error")
                            ),
                        )
                    } else if review_unable {
                        (
                            crate::core::planning_work::WorkStatus::NeedsAttention,
                            "ReviewUnable: the reviewer could not assess this plan from the available evidence. A bounded retry is available; task generation remains blocked.".into(),
                        )
                    } else {
                        (
                            crate::core::planning_work::WorkStatus::NeedsAttention,
                            "Blocked: the independent PlanReviewer requested changes. Resolve findings in the Manager, revise the plan, and obtain fresh user approval.".into(),
                        )
                    };
                    if approved
                        && lifecycle_error.is_none()
                        && verification_error.is_none()
                        && arbitration_error.is_none()
                    {
                        format!(
                            "Independent PlanReviewer approved the exact plan snapshot for {}.",
                            outcome.feature_id
                        )
                    } else if approved && arbitration_error.is_some() {
                        let error = arbitration_error.as_deref().unwrap_or("unknown error");
                        format!(
                            "Arbitration approved the plan, but its Board status could not be saved: {error}. Task generation remains blocked."
                        )
                    } else if approved && lifecycle_error.is_some() {
                        let error = lifecycle_error.as_deref().unwrap_or("unknown error");
                        project.activity.pending.push(format!(
                            "PlanReviewer approved {}, but task generation remains blocked because the lifecycle record could not be synchronized: {error}", outcome.feature_id
                        ));
                        format!(
                            "Plan review was approved, but lifecycle synchronization failed: {error}. Task generation remains blocked."
                        )
                    } else if approved {
                        let error = verification_error.as_deref().unwrap_or("unknown error");
                        project.activity.pending.push(format!(
                            "PlanReviewer approved {}, but Manager repair verification could not be saved: {error}. Task generation remains blocked until the board state is refreshed.", outcome.feature_id
                        ));
                        format!(
                            "Plan review was approved, but Manager repair verification failed: {error}. Task generation remains blocked."
                        )
                    } else if review_unable {
                        let limits = if review_error {
                            "the reviewer failed before returning a valid result".to_owned()
                        } else {
                            outcome.verdict.untested.join("; ")
                        };
                        project.activity.pending.push(format!(
                            "Independent PlanReviewer could not complete a defensible review of {}: {}. Retry is bounded; no task generation is authorized.",
                            outcome.feature_id, limits
                        ));
                        format!(
                            "Independent plan review for {} was unable to complete: {}. You can retry the review; task generation remains blocked.",
                            outcome.feature_id, limits
                        )
                    } else {
                        let findings = outcome
                            .verdict
                            .findings
                            .iter()
                            .map(|finding| {
                                format!(
                                    "{} ({}): {}",
                                    finding.id, finding.requirement, finding.proposed_correction
                                )
                            })
                            .collect::<Vec<_>>();
                        let findings = if findings.is_empty() {
                            "Reviewer did not approve the plan; no detailed findings were returned."
                                .to_owned()
                        } else {
                            findings.join("; ")
                        };
                        project.activity.pending.push(format!(
                            "Independent PlanReviewer requested changes for {}: {}. Resolve findings in the Manager, revise the plan, and obtain fresh user approval before another review.", outcome.feature_id, findings
                        ));
                        format!(
                            "Independent PlanReviewer found issues in {}: {}. Task generation remains blocked; the Manager must resolve the findings and the revised plan must be reapproved.",
                            outcome.feature_id, findings
                        )
                    }
                }
                Err(error) => {
                    if outcome.arbitration.is_some() {
                        lifecycle::mark_arbitration_failed(
                            project,
                            &outcome.feature_id,
                            &error.to_string(),
                        );
                    }
                    task_gate = (
                        crate::core::planning_work::WorkStatus::NeedsAttention,
                        format!(
                            "Blocked: PlanReviewer result was not saved ({error}); task generation remains blocked."
                        ),
                    );
                    format!(
                        "Plan review result was not saved: {error}. Task generation remains blocked."
                    )
                }
            }
        }
    };
    {
        let (status, detail) = task_gate;
        if let Some((_, markdown)) = reviewed_feature.as_deref().and_then(|feature_id| {
            project
                .state
                .active_features
                .iter()
                .find(|(id, _)| id == feature_id)
        }) && let Ok(Some(identity)) = crate::domain::ArtifactIdentity::from_markdown(markdown)
            && let Some(work) = project.planning_work.iter_mut().find(|work| {
                work.kind == crate::core::planning_work::WorkKind::TaskGeneration
                    && work.feature_uid.as_deref() == Some(identity.uid.as_str())
            })
        {
            work.status = status;
            work.detail = detail;
            if let Err(error) = project.save_planning_work() {
                project.activity.pending.push(format!(
                            "Plan review finished, but its task-generation status could not be saved: {error}"
                        ));
            }
        }
    }
    project.remember_chat(vec![crate::domain::chatlog::ChatMessage::new(
        crate::domain::chatlog::ChatRole::System,
        message,
        None,
    )]);
}
