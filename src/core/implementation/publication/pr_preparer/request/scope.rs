use super::super::*;
use super::CHUNK_BYTES;
use crate::core::lifecycle::{FeatureGoal, TaskPhase, TaskRecord};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

const MAX_DIFF_BYTES: u64 = 48 * 1024;
pub(in crate::core::implementation::publication::pr_preparer) fn collect_scope(
    state: &Implementation,
    dir: &Path,
    runner: &Runner,
    task: &TaskRecord,
    included: &[TaskRecord],
    feature: &FeatureGoal,
) -> anyhow::Result<ProposalScope> {
    let head = runner.git(&state.task_repository, &["rev-parse", "HEAD"])?;
    anyhow::ensure!(
        state.verified_head.as_deref() == Some(head.as_str()),
        "integrated head changed before PR preparation"
    );
    let branch = runner.git(&state.task_repository, &["symbolic-ref", "--short", "HEAD"])?;
    anyhow::ensure!(
        branch == state.branch,
        "integrated branch changed before PR preparation"
    );
    let base = runner.git(
        &state.task_repository,
        &[
            "rev-parse",
            "--verify",
            &format!("{}^{{commit}}", state.base_commit),
        ],
    )?;
    anyhow::ensure!(base == state.base_commit, "PR base commit is not canonical");
    let commits = runner
        .git(
            &state.task_repository,
            &["rev-list", "--reverse", &format!("{base}..{head}")],
        )?
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    anyhow::ensure!(!commits.is_empty(), "PR scope has no commits");
    let paths_file = dir.join("pr-preparer-paths.tmp");
    let paths = capture_git(
        runner,
        &state.task_repository,
        &[
            "diff",
            "--name-only",
            "-z",
            &format!("{base}...{head}"),
            "--",
        ],
        &paths_file,
        None,
    )?;
    let files = paths
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| {
            String::from_utf8(path.to_vec())
                .map_err(|_| anyhow::anyhow!("PR file path is not valid UTF-8"))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    anyhow::ensure!(!files.is_empty(), "PR scope has no changed files");
    let diff = capture_git(
        runner,
        &state.task_repository,
        &[
            "diff",
            "--no-ext-diff",
            "--no-color",
            &format!("{base}...{head}"),
            "--",
        ],
        &dir.join("pr-preparer-diff.tmp"),
        Some(MAX_DIFF_BYTES),
    )?;
    let diff = String::from_utf8(diff)
        .map_err(|_| anyhow::anyhow!("PR Preparer requires a UTF-8 integrated diff"))?;
    let feature_plan = format!(
        "Intent:\n{}\n\nApproved draft:\n{}",
        feature.original_intent, feature.draft
    );
    let task_spec = format!(
        "Task:\n{}\n\nApproved task specification:\n{}",
        state.ticket_text,
        state
            .approved_specification
            .as_deref()
            .unwrap_or(&state.ticket_text)
    );
    let gate_evidence = serde_json::to_string(
        &included
            .iter()
            .map(|record| TaskGateEvidence {
                task_uid: &record.task_uid,
                revision: record.revision,
                phase: record.phase,
                approved_plan_sha256: &record.approved_plan_sha256,
                implementation_sha256: record.implementation_sha256.as_deref(),
                reviewed_code_head_sha: record.reviewed_code_head_sha.as_deref(),
                reviewed_code_subject_sha256: record.reviewed_code_subject_sha256.as_deref(),
                code_review: &record.code_review,
                runtime_qa: &record.runtime_qa,
                spec_reconciliation: &record.spec_reconciliation,
            })
            .collect::<Vec<_>>(),
    )?;
    let verified_report = fs::read_to_string(dir.join("verified-report.json"))?;
    anyhow::ensure!(
        !verified_report.trim().is_empty(),
        "PR Preparer requires the application-validated verification report"
    );

    let base_ref = format!("refs/heads/{}", state.base);
    let head_ref = format!("refs/heads/{}", state.branch);
    let mut evidence_ids = Vec::new();
    extend_chunk_ids(
        &mut evidence_ids,
        "approved-feature-plan",
        feature_plan.len(),
    );
    extend_chunk_ids(&mut evidence_ids, "approved-task-spec", task_spec.len());
    extend_chunk_ids(&mut evidence_ids, "lifecycle-gates", gate_evidence.len());
    extend_chunk_ids(
        &mut evidence_ids,
        "verified-test-report",
        verified_report.len(),
    );
    extend_chunk_ids(&mut evidence_ids, "integrated-diff", diff.len());
    let mut manifest_chunks = 1usize;
    let (scope_sha256, scope_manifest) = loop {
        let mut ids = evidence_ids.clone();
        ids.extend((0..manifest_chunks).map(|index| format!("pr-scope-manifest-{index}")));
        let expected = ExpectedProposal {
            task_uid: task.task_uid.clone(),
            base_ref: base_ref.clone(),
            head_ref: head_ref.clone(),
            commits: commits.clone(),
            files: files.clone(),
            evidence_ids: ids,
        };
        let hash_input = serde_json::to_vec(&(
            &expected,
            &base,
            &head,
            &feature_plan,
            &task_spec,
            &gate_evidence,
            &verified_report,
            &diff,
        ))?;
        let scope_sha256 = format!("{:x}", Sha256::digest(hash_input));
        let manifest = serde_json::json!({
            "task_uid": expected.task_uid,
            "base_ref": expected.base_ref,
            "head_ref": expected.head_ref,
            "commits": expected.commits,
            "files": expected.files,
            "evidence": expected.evidence_ids,
            "scope_sha256": scope_sha256,
        })
        .to_string();
        let actual_chunks = chunk_count(manifest.len());
        if actual_chunks == manifest_chunks {
            break (scope_sha256, manifest);
        }
        manifest_chunks = actual_chunks;
    };
    evidence_ids.extend((0..manifest_chunks).map(|index| format!("pr-scope-manifest-{index}")));
    Ok(ProposalScope {
        expected: ExpectedProposal {
            task_uid: task.task_uid.clone(),
            base_ref,
            head_ref,
            commits,
            files,
            evidence_ids,
        },
        scope_sha256,
        feature_plan,
        task_spec,
        gate_evidence,
        verified_report,
        diff,
        scope_manifest,
    })
}
pub(in crate::core::implementation::publication::pr_preparer) fn validate_included_tasks(
    parent: &TaskRecord,
    tasks: &[TaskRecord],
) -> anyhow::Result<()> {
    anyhow::ensure!(
        tasks.iter().any(|task| task.task_uid == parent.task_uid)
            && tasks.len() == parent.subtask_uids.len() + 1,
        "PR preparation scope does not contain the parent and its full subtask set"
    );
    for child_uid in &parent.subtask_uids {
        let child = tasks
            .iter()
            .find(|task| task.task_uid == *child_uid)
            .ok_or_else(|| anyhow::anyhow!("PR preparation scope omits an approved subtask"))?;
        anyhow::ensure!(
            child.phase == TaskPhase::Done
                && child.parent_task_uid.as_deref() == Some(parent.task_uid.as_str())
                && child.feature_uid == parent.feature_uid,
            "PR preparation includes a subtask without completed parent ownership"
        );
    }
    Ok(())
}

fn capture_git(
    runner: &Runner,
    repo: &Path,
    args: &[&str],
    path: &Path,
    max_bytes: Option<u64>,
) -> anyhow::Result<Vec<u8>> {
    let _cleanup = TemporaryCapture(path.to_owned());
    if let Some(limit) = max_bytes {
        runner.git_to_file_bounded(repo, args, path, limit)?;
    } else {
        runner.git_to_file(repo, args, path)?;
    }
    Ok(fs::read(path)?)
}

struct TemporaryCapture(PathBuf);

impl Drop for TemporaryCapture {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn extend_chunk_ids(ids: &mut Vec<String>, prefix: &str, bytes: usize) {
    ids.extend((0..chunk_count(bytes)).map(|index| format!("{prefix}-{index}")));
}

fn chunk_count(bytes: usize) -> usize {
    bytes.max(1).div_ceil(CHUNK_BYTES)
}
