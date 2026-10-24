use crate::{app::session::Project, core::lifecycle::TaskRecord};
use sha2::Digest as _;

pub(super) fn for_task(
    project: &Project,
    task: &TaskRecord,
) -> (
    Option<crate::ui::task_detail::RuntimeQaEvidenceView>,
    Option<String>,
) {
    let Some(request) = task
        .qa_execution
        .as_ref()
        .and_then(|execution| execution.request.as_ref())
    else {
        return (None, None);
    };
    if task.qa_execution.as_ref().is_none_or(|execution| {
        execution.status != crate::core::lifecycle::QaExecutionStatusV1::Completed
    }) {
        return (None, None);
    }
    let root = match crate::runtime_qa::RuntimeQaStore::local_root_for_workspace(
        project.state.planning_store.project_id,
    ) {
        Ok(root) => root,
        Err(error) => return (None, Some(error)),
    };
    let store = crate::runtime_qa::RuntimeQaStore::new(root);
    let run = match store.load_run_with_screenshots(&request.qa_run_id) {
        Ok(run) => run,
        Err(error) => return (None, Some(error)),
    };
    let persisted_request = run.request;
    let evidence = run.evidence;
    let result = run.result;
    let artifacts = run.screenshots;
    let result_sha = match serde_json::to_vec(&result) {
        Ok(encoded) => format!("{:x}", sha2::Sha256::digest(encoded)),
        Err(error) => {
            return (
                None,
                Some(format!("cannot verify runtime QA result: {error}")),
            );
        }
    };
    let evidence_sha = match evidence.sha256() {
        Ok(encoded) => encoded,
        Err(error) => return (None, Some(error)),
    };
    let execution = task.qa_execution.as_ref();
    if persisted_request.parent_task_uid != task.task_uid
        || persisted_request != *request
        || !receipt_hashes_match(
            execution.and_then(|execution| execution.result_sha256.as_deref()),
            execution.and_then(|execution| execution.evidence_manifest_sha256.as_deref()),
            &result_sha,
            &evidence_sha,
        )
        || result.evidence_manifest_sha256 != evidence_sha
    {
        return (
            None,
            Some("persisted runtime QA evidence does not match this task receipt".into()),
        );
    }
    let screenshots = evidence
        .screenshots
        .iter()
        .map(|metadata| {
            let artifact = artifacts
                .iter()
                .find(|artifact| artifact.id == metadata.id)
                .ok_or_else(|| "runtime QA screenshot bytes are missing".to_owned())?;
            Ok(crate::ui::task_detail::RuntimeQaScreenshotView {
                id: metadata.id.clone(),
                scenario_id: metadata.scenario_id.clone(),
                sha256: metadata.sha256.clone(),
                width: metadata.width,
                height: metadata.height,
                bytes: artifact.bytes.clone(),
            })
        })
        .collect::<Result<Vec<_>, String>>();
    let screenshots = match screenshots {
        Ok(screenshots) => screenshots,
        Err(error) => return (None, Some(error)),
    };
    (
        Some(crate::ui::task_detail::RuntimeQaEvidenceView {
            run_id: request.qa_run_id.clone(),
            outcome: result.outcome,
            target_id: evidence.target_id.clone(),
            code_sha: evidence.code_sha.clone(),
            approved_spec_hash: evidence.approved_spec_hash.clone(),
            build_hash: evidence.build_hash.clone(),
            platform: evidence.platform,
            driver_version: evidence.driver_version.clone(),
            display: evidence.display.clone(),
            coverage: result.coverage.requirements,
            review_coverage: result.review_coverage,
            reviewed_screenshot_ids: result.reviewed_screenshot_ids,
            findings: result.findings,
            assertions: evidence.assertions,
            actions: evidence.actions,
            screenshots,
        }),
        None,
    )
}

fn receipt_hashes_match(
    receipt_result_sha: Option<&str>,
    receipt_evidence_sha: Option<&str>,
    persisted_result_sha: &str,
    persisted_evidence_sha: &str,
) -> bool {
    receipt_result_sha == Some(persisted_result_sha)
        && receipt_evidence_sha == Some(persisted_evidence_sha)
}

#[cfg(test)]
mod tests {
    use super::receipt_hashes_match;

    #[test]
    fn task_details_rejects_manifest_hash_mismatch_when_result_hash_matches() {
        assert!(!receipt_hashes_match(
            Some("result-sha"),
            Some("stale-evidence-sha"),
            "result-sha",
            "persisted-evidence-sha",
        ));
    }
}
