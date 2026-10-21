use super::*;
use crate::harness::prompt_catalog::{
    PrProposalV1, PromptRole, RoleInputV1, RolePayloadV1, render,
};

fn expected() -> ExpectedProposal {
    ExpectedProposal {
        task_uid: "task-1".into(),
        base_ref: "refs/heads/main".into(),
        head_ref: "refs/heads/koolade/task-1".into(),
        commits: vec!["head-sha".into()],
        files: vec!["src/main.rs".into(), "tests/main.rs".into()],
        evidence_ids: vec!["approved-feature-plan-0".into(), "integrated-diff-0".into()],
    }
}

fn proposal(expected: &ExpectedProposal) -> PrProposalV1 {
    PrProposalV1 {
        task_uid: expected.task_uid.clone(),
        base_ref: expected.base_ref.clone(),
        head_ref: expected.head_ref.clone(),
        commits: expected.commits.clone(),
        files: expected.files.clone(),
        evidence: expected.evidence_ids.clone(),
        description: "Summarize the reviewed task changes.".into(),
    }
}

fn scope() -> ProposalScope {
    ProposalScope {
        expected: expected(),
        scope_sha256: "a".repeat(64),
        feature_plan: String::new(),
        task_spec: String::new(),
        gate_evidence: String::new(),
        verified_report: String::new(),
        diff: String::new(),
        scope_manifest: String::new(),
    }
}

fn receipt(scope: &ProposalScope) -> PreparedProposalV1 {
    let prompt = render(&RoleInputV1 {
        role: PromptRole::PrPreparer,
        workspace_uid: "workspace-1".into(),
        feature_uid: "feature-1".into(),
        task_uid: Some(scope.expected.task_uid.clone()),
        original_goal: "Prepare the approved task for review".into(),
        approved_spec_ref: None,
        approved_spec_hash: None,
        decision_refs: Vec::new(),
        evidence_manifest: Vec::new(),
        execution_policy_ref: "publication/pr-preparer/read-only-exact-scope/v1".into(),
        output_schema_version: PromptRole::PrPreparer.output_schema_version().into(),
        payload: RolePayloadV1::Pr {
            reviewed_task_refs: Vec::new(),
            approved_subtasks: Vec::new(),
            base_ref: scope.expected.base_ref.clone(),
            head_ref: scope.expected.head_ref.clone(),
        },
    })
    .unwrap()
    .manifest;
    PreparedProposalV1 {
        schema_version: 1,
        scope_sha256: scope.scope_sha256.clone(),
        prompt,
        proposal: proposal(&scope.expected),
    }
}

#[test]
fn pr_preparer_requires_exact_application_scope_and_evidence() {
    let expected = expected();
    let proposal = proposal(&expected);
    assert!(validate_proposal(&proposal, &expected).is_ok());

    let mut wrong_head = proposal.clone();
    wrong_head.head_ref = "refs/heads/unreviewed".into();
    assert!(validate_proposal(&wrong_head, &expected).is_err());

    let mut missing_file = proposal.clone();
    missing_file.files.pop();
    assert!(validate_proposal(&missing_file, &expected).is_err());

    let mut unsupported_evidence = proposal;
    unsupported_evidence.evidence[0] = "model-invented-test-result".into();
    assert!(validate_proposal(&unsupported_evidence, &expected).is_err());
}

#[test]
fn pr_preparer_rejects_duplicate_evidence_and_empty_description() {
    let expected = expected();
    let mut duplicate_evidence = proposal(&expected);
    duplicate_evidence.evidence[1] = duplicate_evidence.evidence[0].clone();
    assert!(validate_proposal(&duplicate_evidence, &expected).is_err());

    let mut empty_description = proposal(&expected);
    empty_description.description = "  ".into();
    assert!(validate_proposal(&empty_description, &expected).is_err());
}

#[test]
fn pr_preparer_reuses_only_current_scope_and_prompt_receipts() {
    let scope = scope();
    let valid = receipt(&scope);
    assert!(validate_receipt(&valid, &scope).unwrap());

    let mut stale_scope = valid.clone();
    stale_scope.scope_sha256 = "b".repeat(64);
    assert!(!validate_receipt(&stale_scope, &scope).unwrap());

    let mut stale_schema = valid.clone();
    stale_schema.schema_version = 2;
    assert!(!validate_receipt(&stale_schema, &scope).unwrap());

    let mut stale_prompt = valid.clone();
    stale_prompt.prompt.role_version = "pr/v99".into();
    assert!(!validate_receipt(&stale_prompt, &scope).unwrap());

    let mut corrupted = valid;
    corrupted.proposal.files.pop();
    assert!(validate_receipt(&corrupted, &scope).is_err());
}

#[test]
fn stale_pr_preparer_receipts_are_removed_so_restart_can_rebuild_them() {
    let scope = scope();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("pr-preparer-v1.json");
    let mut stale_scope = receipt(&scope);
    stale_scope.scope_sha256 = "b".repeat(64);
    fs::write(&path, serde_json::to_vec(&stale_scope).unwrap()).unwrap();
    assert!(read_reusable_receipt(&path, &scope).unwrap().is_none());
    assert!(!path.exists());

    let mut stale_prompt = receipt(&scope);
    stale_prompt.prompt.role_version = "pr/v99".into();
    fs::write(&path, serde_json::to_vec(&stale_prompt).unwrap()).unwrap();
    assert!(read_reusable_receipt(&path, &scope).unwrap().is_none());
    assert!(!path.exists());

    let current = receipt(&scope);
    fs::write(&path, serde_json::to_vec(&current).unwrap()).unwrap();
    assert!(read_reusable_receipt(&path, &scope).unwrap().is_some());
    assert!(path.exists());
}

// Production boundary: the persisted PR Preparer receipt must materialize the
// body file consumed by the provider controller, including after restart. A
// string-rendering unit test cannot prove replacement/recovery ordering. Uses
// one temporary directory, no network/provider/Docker; negligible setup.
#[test]
fn publication_body_uses_current_validated_proposal_after_restart() {
    let mut scope = scope();
    let private_marker = "SYNTHETIC_PRIVATE_CONTEXT_7f2a";
    scope.feature_plan = format!("private planning note: {private_marker}");
    scope.task_spec = format!("private task context: {private_marker}");
    scope.gate_evidence = format!("private gate output: {private_marker}");
    scope.verified_report = format!("private report: {private_marker}");
    scope.diff = format!("private adjacent diff: {private_marker}");
    scope.scope_manifest = format!("private manifest context: {private_marker}");
    let current = receipt(&scope);
    let temp = tempfile::tempdir().unwrap();
    let receipt_path = temp.path().join("pr-preparer-v1.json");
    fs::write(&receipt_path, serde_json::to_vec(&current).unwrap()).unwrap();
    let body_path = temp.path().join("pr-body.md");
    fs::write(&body_path, "Old verification-only summary").unwrap();
    let reopened = read_reusable_receipt(&receipt_path, &scope)
        .unwrap()
        .unwrap();
    super::body::write(temp.path(), &reopened, &scope).unwrap();
    let body = fs::read_to_string(&body_path).unwrap();
    assert!(body.starts_with(&current.proposal.description));
    assert!(body.contains(&scope.scope_sha256));
    assert!(body.contains("src/main.rs"));
    assert!(!body.contains("Old verification-only summary"));
    assert!(!body.contains(private_marker));

    let mut stale = current;
    stale.scope_sha256 = "b".repeat(64);
    assert!(super::body::write(temp.path(), &stale, &scope).is_err());
    assert_eq!(fs::read_to_string(body_path).unwrap(), body);
}

#[test]
fn publication_body_rejects_private_values_from_the_scoped_evidence() {
    let mut scope = scope();
    scope.feature_plan = "Customer contact: synthetic.user@example.invalid".into();
    let mut prepared = receipt(&scope);
    prepared.proposal.description =
        "Coordinate follow-up with synthetic.user@example.invalid".into();
    let temp = tempfile::tempdir().unwrap();

    let error = super::body::write(temp.path(), &prepared, &scope).unwrap_err();

    assert!(error.to_string().contains("sensitive value"));
    assert!(!temp.path().join("pr-body.md").exists());
}

#[test]
fn publication_body_rejects_model_generated_private_values_without_source_echo() {
    let mut scope = scope();
    scope.feature_plan = "Adds an account recovery workflow.".into();
    let mut prepared = receipt(&scope);
    prepared.proposal.description =
        "Adds recovery instructions; contact synthetic.user@example.invalid for help.".into();
    let temp = tempfile::tempdir().unwrap();

    let error = super::body::write(temp.path(), &prepared, &scope).unwrap_err();

    assert!(error.to_string().contains("sensitive value"));
    assert!(super::privacy::is_sensitive_content_error(&error));
    assert!(!temp.path().join("pr-body.md").exists());
}

#[test]
fn publication_body_rejects_model_generated_spaced_phone_number() {
    let mut scope = scope();
    scope.feature_plan = "Adds an account recovery workflow.".into();
    let mut prepared = receipt(&scope);
    prepared.proposal.description =
        "Adds recovery instructions; call 202 555 0199 for help.".into();
    let temp = tempfile::tempdir().unwrap();

    let error = super::body::write(temp.path(), &prepared, &scope).unwrap_err();

    assert!(super::privacy::is_sensitive_content_error(&error));
    assert!(!temp.path().join("pr-body.md").exists());
}

#[test]
fn publication_body_rejects_lowercase_name_after_context_marker() {
    let mut scope = scope();
    scope.feature_plan = "The customer jane doe asked for a change.".into();
    let mut prepared = receipt(&scope);
    prepared.proposal.description = "Adds the change requested by jane doe.".into();
    let temp = tempfile::tempdir().unwrap();

    let error = super::body::write(temp.path(), &prepared, &scope).unwrap_err();

    assert!(super::privacy::is_sensitive_content_error(&error));
    assert!(!temp.path().join("pr-body.md").exists());
}

#[test]
fn publication_body_rejects_private_values_in_rendered_scope_metadata() {
    let mut scope = scope();
    scope.feature_plan = "Customer contact: synthetic.user@example.invalid".into();
    scope.expected.head_ref = "refs/heads/synthetic.user@example.invalid".into();
    let prepared = receipt(&scope);
    let temp = tempfile::tempdir().unwrap();

    let error = super::body::write(temp.path(), &prepared, &scope).unwrap_err();

    assert!(error.to_string().contains("sensitive value"));
    assert!(!temp.path().join("pr-body.md").exists());
}
