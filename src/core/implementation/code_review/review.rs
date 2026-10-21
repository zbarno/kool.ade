use super::*;

pub(super) fn review_verified_inner(
    dir: &Path,
    state: &Implementation,
    lifecycle_task: Option<&crate::core::lifecycle::TaskRecord>,
    harness: &dyn AiHarness,
    runner: &Runner,
    arbitration: Option<&CodeReviewArbitrationContext>,
    arbitration_author: Option<&ReviewExecutionIdentity>,
) -> anyhow::Result<()> {
    let head = state.verified_head.as_deref().ok_or_else(|| {
        anyhow::anyhow!("Independent code review is blocked: verified task head is missing")
    })?;
    let task_uid = state.task_uid.as_deref().ok_or_else(|| {
        anyhow::anyhow!("Independent code review is blocked: stable task identity is missing")
    })?;
    let feature_uid = state
        .ticket_text
        .lines()
        .find_map(|line| line.strip_prefix("Feature: "))
        .and_then(|value| {
            crate::core::workflow::feature_ids_in(value)
                .into_iter()
                .next()
        })
        .ok_or_else(|| {
            anyhow::anyhow!("Independent code review is blocked: feature identity is missing")
        })?;
    let spec = state
        .approved_specification
        .as_deref()
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Independent code review is blocked: approved task specification is missing"
            )
        })?;
    let author = arbitration_author
        .cloned()
        .or_else(|| harness.current_execution_identity())
        .ok_or_else(|| {
        anyhow::anyhow!("Independent code review is blocked: controller author run/session receipt is unavailable")
    })?;
    ensure_identity(&author, "author")?;
    anyhow::ensure!(
        git_head(runner, &state.task_repository)? == head,
        "Independent code review is blocked: task clone head changed after verification"
    );
    anyhow::ensure!(
        runner
            .git(&state.task_repository, &["status", "--porcelain"])?
            .trim()
            .is_empty(),
        "Independent code review is blocked: task clone has uncommitted changes outside the immutable diff"
    );

    let diff_path = dir.join("code-review-subject.patch");
    runner.git_to_file_bounded(
        &state.task_repository,
        &[
            "diff",
            "--binary",
            &format!("{}...{}", state.base_commit, head),
        ],
        &diff_path,
        MAX_DIFF_BYTES,
    )?;
    let diff = fs::read(&diff_path)?;
    let diff_text = String::from_utf8(diff.clone()).map_err(|_| {
        anyhow::anyhow!(
            "Independent code review is blocked: complete diff is not UTF-8 review evidence"
        )
    })?;
    // A verified no-op task still receives an independent verdict. The empty
    // diff is itself the exact review subject; the CodeReviewer also sees the
    // approved criteria and validated test receipt before it can pass.
    let diff_hash = digest(&diff);
    let paths = runner
        .git(
            &state.task_repository,
            &[
                "diff",
                "--name-only",
                "-z",
                &format!("{}...{}", state.base_commit, head),
            ],
        )?
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let requirements = requirement_matrix(&state.ticket_text, task_uid);
    let linked_subtask_ids = lifecycle_task
        .map(|task| task.subtask_uids.clone())
        .unwrap_or_default();
    let build_artifacts = lifecycle_task
        .and_then(|task| task.qa_execution.as_ref())
        .and_then(|qa| qa.request.as_ref())
        .map(|request| vec![request.build_artifact_sha256.clone()])
        .unwrap_or_default();
    let runtime_qa_receipt_ids = lifecycle_task
        .and_then(|task| task.runtime_qa.as_ref())
        .map(|receipt| vec![receipt.receipt_uid.clone()])
        .unwrap_or_default();
    let existing_review_attempt_ids = lifecycle_task
        .map(|task| {
            task.review_history
                .iter()
                .map(|receipt| receipt.receipt_uid.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let approved_plan_hash = lifecycle_task
        .map(|task| task.approved_plan_sha256.clone())
        .unwrap_or_else(|| digest(spec.as_bytes()));
    let task_spec_revision = lifecycle_task
        .and_then(|task| task.task_spec_sha256.as_deref())
        .unwrap_or(&approved_plan_hash);
    let report_path = dir.join("verified-report.json");
    let report = fs::read_to_string(report_path).unwrap_or_default();
    anyhow::ensure!(
        !report.trim().is_empty(),
        "Independent code review is blocked: validated test execution receipts are missing"
    );
    let test_execution_receipts = vec![digest(report.as_bytes())];
    let subject = ReviewSubject {
        task_uid,
        feature_uid: &feature_uid,
        approved_plan_hash: &approved_plan_hash,
        task_spec_revision,
        task_clone_head_sha: head,
        base_sha: &state.base_commit,
        full_diff_hash: &diff_hash,
        linked_subtask_ids: &linked_subtask_ids,
        affected_paths: &paths,
        requirement_matrix: &requirements,
        test_execution_receipts: &test_execution_receipts,
        build_artifacts: &build_artifacts,
        runtime_qa_receipt_ids: &runtime_qa_receipt_ids,
        existing_review_attempt_ids: &existing_review_attempt_ids,
    };
    let subject_hash = digest(&serde_json::to_vec(&subject)?);
    if let Some(receipt) = load_receipt(dir, task_uid)? {
        if receipt.subject_hash == subject_hash
            && receipt.diff_hash == diff_hash
            && receipt.feature_uid == feature_uid
            && receipt.base_sha == state.base_commit
            && receipt.head_sha == head
            && receipt.author_run_id == author.execution_id
            && receipt.author_session_id == author.session_id
            && uses_current_prompt(&receipt)
            && is_approved_verdict(&receipt.verdict)
        {
            return Ok(());
        }
        if same_code_failed_review_requires_manager_resolution(
            &receipt,
            &state.base_commit,
            head,
            &diff_hash,
            arbitration.is_some(),
        ) {
            anyhow::bail!(
                "Independent code review did not pass; Manager resolution is required before retry. Findings are persisted in {}",
                dir.join(RECEIPT_FILE).display()
            );
        }
    }

    let reviewer = ReviewExecutionIdentity {
        execution_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        session_id: uuid::Uuid::new_v4().hyphenated().to_string(),
    };
    ensure_identity(&reviewer, "reviewer")?;
    anyhow::ensure!(
        reviewer.execution_id != author.execution_id && reviewer.session_id != author.session_id,
        "Independent code review is blocked: reviewer identity is not independent"
    );
    let (request, manifest, evidence_ids) = build_request(CodeReviewRequestInput {
        state,
        feature_uid: &feature_uid,
        subject_hash: &subject_hash,
        approved_plan_hash: &approved_plan_hash,
        diff: &diff_text,
        spec,
        report: &report,
        requirements: &requirements,
        subject_context: &serde_json::to_string(&subject)?,
        author: &author,
        reviewer: &reviewer,
        arbitration,
    })?;
    runner.update("Running independent read-only CodeReviewer on the frozen task diff…");
    let output = harness.execute_review(&request, &reviewer)?;
    let context = crate::harness::prompt_catalog::OutputValidationContext {
        expected_subject_hash: Some(&subject_hash),
        evidence_ids: evidence_ids.clone(),
        required_requirements: requirements.clone(),
        ..Default::default()
    };
    let verdict = match crate::harness::prompt_catalog::validate_output_with_context(
        crate::harness::prompt_catalog::PromptRole::CodeReviewer,
        &output.final_text,
        &context,
    )
    .map_err(anyhow::Error::msg)?
    {
        crate::harness::prompt_catalog::RoleOutputV1::Review(verdict) => verdict,
        _ => anyhow::bail!("CodeReviewer returned an unexpected output schema"),
    };
    let mut examined_files = verdict.examined_files.clone();
    let mut affected_paths = paths.clone();
    examined_files.sort();
    affected_paths.sort();
    anyhow::ensure!(
        examined_files == affected_paths,
        "Independent code review must enumerate every affected file in the frozen diff"
    );
    anyhow::ensure!(
        verdict.findings.iter().all(|finding| {
            finding
                .location_detail
                .as_ref()
                .is_some_and(|location| paths.contains(&location.file))
        }),
        "Independent code review finding location is outside the frozen diff"
    );
    anyhow::ensure!(
        !verdict.pass
            || verdict.coverage.iter().all(|coverage| coverage
                .evidence
                .iter()
                .any(|id| id.starts_with("verified-test-report-"))),
        "Independent code review did not cite the application-validated test receipt for each acceptance check"
    );
    let receipt = CodeReviewReceiptV1 {
        schema_version: 1,
        task_uid: task_uid.to_owned(),
        feature_uid,
        subject_hash,
        diff_hash,
        base_sha: state.base_commit.clone(),
        head_sha: head.to_owned(),
        author_run_id: author.execution_id,
        author_session_id: author.session_id,
        reviewer_run_id: reviewer.execution_id,
        reviewer_session_id: reviewer.session_id,
        prompt: manifest,
        requirements,
        affected_paths: paths,
        evidence_ids: evidence_ids.into_iter().collect(),
        verdict,
    };
    validate_receipt(&receipt)?;
    crate::artifacts::atomic_write_bytes(
        &dir.join(RECEIPT_FILE),
        &serde_json::to_vec_pretty(&receipt)?,
    )?;
    if !is_approved_verdict(&receipt.verdict) {
        anyhow::bail!(
            "Independent code review found blocking issues; Manager resolution is required. Findings are persisted in {}",
            dir.join(RECEIPT_FILE).display()
        );
    }
    Ok(())
}
