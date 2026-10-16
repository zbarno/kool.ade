use crate::{
    artifacts::planning_store::PlanningStore,
    core::lifecycle::{GateActor, GateDecision, GateReceipt},
};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Persisted<T> {
    pub(super) uid: String,
    pub(super) schema_version: u16,
    pub(super) created_at_ms: u64,
    pub(super) updated_at_ms: u64,
    pub(super) revision: u64,
    pub(super) record: T,
}

pub(super) fn code_review_receipt(
    store: &PlanningStore,
    feature_uid: &str,
    snapshot_uid: &str,
    task_uid: &str,
    approved_hash: &str,
    implementation_hash: &str,
) -> GateReceipt {
    use crate::harness::prompt_catalog::{PromptRole, RoleInputV1, RolePayloadV1, render};
    let author_run = "00000000-0000-4000-8000-000000000101".to_owned();
    let reviewer_run = "00000000-0000-4000-8000-000000000102".to_owned();
    let author_session = "00000000-0000-4000-8000-000000000103".to_owned();
    let reviewer_session = "00000000-0000-4000-8000-000000000104".to_owned();
    let role = PromptRole::CodeReviewer;
    let manifest = render(&RoleInputV1 {
        role,
        workspace_uid: store.project_id.hyphenated().to_string(),
        feature_uid: feature_uid.into(),
        task_uid: Some(task_uid.into()),
        original_goal: "Verify synthetic trace fixture implementation".into(),
        approved_spec_ref: Some(snapshot_uid.into()),
        approved_spec_hash: Some(approved_hash.into()),
        decision_refs: Vec::new(),
        evidence_manifest: vec![crate::harness::prompt_catalog::EvidenceItem {
            id: "fixture-review-diff".into(),
            workspace_uid: store.project_id.hyphenated().to_string(),
            origin: crate::harness::prompt_catalog::EvidenceOrigin::Repository,
            confidentiality:
                crate::harness::prompt_catalog::EvidenceConfidentiality::WorkspacePrivate,
            source: "fixture/review-diff".into(),
            content: "synthetic review evidence".into(),
        }],
        execution_policy_ref: "trace-review-fixture/v1".into(),
        output_schema_version: role.output_schema_version().into(),
        payload: RolePayloadV1::Review {
            subject_revision: "fixture-code-revision-1".into(),
            subject_hash: implementation_hash.into(),
            author_run_id: author_run.clone(),
            reviewer_run_id: reviewer_run.clone(),
            author_session_id: Some(author_session.clone()),
            reviewer_session_id: Some(reviewer_session.clone()),
            requirements_matrix: vec!["Resolve each reference to its requirement".into()],
            explicit_exclusions: Vec::new(),
        },
    })
    .unwrap()
    .manifest;
    GateReceipt {
        receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        subject_uid: task_uid.into(),
        subject_sha256: implementation_hash.into(),
        actor: GateActor::IndependentReviewer,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: 1,
        evidence_refs: vec!["fixture:review-diff".into()],
        prompt_sha256: Some(manifest.identity_sha256()),
        prompt_manifest: Some(manifest),
        reviewer_run_id: Some(reviewer_run),
        author_run_id: Some(author_run),
        reviewer_session_id: Some(reviewer_session),
        author_session_id: Some(author_session),
    }
}
