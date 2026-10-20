use crate::app::session::Project;
use crate::core::lifecycle::TaskRecord;

pub(super) fn details(
    project: &Project,
    task: &TaskRecord,
) -> (
    Option<crate::ui::task_detail::CodeReviewInspectionView>,
    Option<String>,
    Vec<crate::ui::task_detail::CodeReviewDispositionView>,
) {
    let (current_code_review, code_review_load_error) = project
        .implementation_states
        .values()
        .find(|state| state.task_uid.as_deref() == Some(task.task_uid.as_str()))
        .map(|state| {
            match crate::core::implementation::manager_findings_for_task(
                &project.state.repo_root,
                &state.ticket,
                &task.task_uid,
            ) {
                Ok(Some(review))
                    if task.reviewed_code_head_sha.as_deref() == Some(review.head_sha.as_str())
                        && task.reviewed_code_subject_sha256.as_deref()
                            == Some(review.subject_hash.as_str()) =>
                {
                    let verdict = crate::harness::prompt_catalog::ReviewVerdictV1 {
                        subject_hash: review.subject_hash.clone(),
                        pass: review.pass,
                        examined_files: review.examined_files,
                        tested_evidence_ids: review.tested_evidence_ids,
                        unexamined_areas: review.unexamined_areas,
                        outcome: review.outcome,
                        coverage: review.coverage,
                        findings: review.findings,
                        untested: review.untested,
                    };
                    (
                        Some(crate::ui::task_detail::CodeReviewInspectionView {
                            task_uid: review.task_uid,
                            subject_hash: review.subject_hash,
                            diff_hash: review.diff_hash,
                            head_sha: review.head_sha,
                            prompt_hash: review.prompt_hash,
                            reviewer_run_id: review.reviewer_run_id,
                            reviewer_session_id: review.reviewer_session_id,
                            verdict,
                        }),
                        None,
                    )
                }
                Ok(_) => (None, None),
                Err(error) => (None, Some(error.to_string())),
            }
        })
        .unwrap_or((None, None));
    let code_review_dispositions = project
        .planning_work
        .iter()
        .filter_map(|work| {
            let disposition = work.code_review_disposition.as_ref()?;
            (work.parent_uid.as_deref() == Some(task.task_uid.as_str())).then(|| {
                crate::ui::task_detail::CodeReviewDispositionView {
                    finding_id: disposition.finding_id.clone(),
                    subject_hash: disposition.subject_hash.clone(),
                    scope: disposition.scope,
                    status: disposition.status,
                    work_id: work.uid.clone(),
                    title: work.title.clone(),
                    work_status: work.status,
                    occurrence_count: disposition.occurrence_count,
                }
            })
        })
        .collect();
    (
        current_code_review,
        code_review_load_error,
        code_review_dispositions,
    )
}
