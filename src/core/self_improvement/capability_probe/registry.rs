use super::super::{CapabilityProbeRegistration, probe_capability_manifest_for_run_with_registry};
use crate::core::self_improvement::{SelfImprovementPhaseV1, SelfImprovementRunV1};

fn keyboard_navigation_probe(run: &SelfImprovementRunV1) -> Result<(), String> {
    if run.capability_id.as_deref() == Some("keyboard-navigation") {
        Err("keyboard-navigation probe invoked".into())
    } else {
        Err("probe received a different capability than the approved plan".into())
    }
}

/// Production boundary: a compiled app-owned registry dispatches the approved
/// capability to its own behavior probe. Manifest parsing alone cannot prove
/// that activation invokes the selected feature probe. Uses an in-process
/// registry and temporary value only; no external dependencies or setup cost.
#[test]
fn registered_feature_probe_can_run_for_another_product_capability() {
    let registry = [CapabilityProbeRegistration {
        capability_id: "keyboard-navigation",
        readiness_probe_id: "keyboard-navigation-v1",
        probe: keyboard_navigation_probe,
    }];
    let source = serde_json::json!({
        "schemaVersion": 1,
        "capabilities": [{
            "capabilityId": "keyboard-navigation",
            "readinessProbeId": "keyboard-navigation-v1"
        }]
    })
    .to_string();
    let run = SelfImprovementRunV1 {
        schema_version: 2,
        run_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        opportunity_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        approved_decision_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        target_repository_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        canonical_remote: "https://github.com/zbarno/kool.ade".into(),
        source_ref: None,
        source_sha: "a".repeat(40),
        destination_ref: None,
        feature_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        capability_id: Some("keyboard-navigation".into()),
        task_uids: vec![],
        owner_task_uid: None,
        parent_task_progress: vec![],
        clone_uid: None,
        plan_approval_hash: None,
        independent_plan_review_uid: None,
        implementation_head_sha: None,
        implementation_subject_sha256: None,
        code_review_uids: vec![],
        runtime_qa_uids: vec![],
        spec_receipt_uid: None,
        pr_work_item_uid: None,
        pr_url: None,
        merged_sha: None,
        installed_version: None,
        installed_build_commit_sha: None,
        activated_version: None,
        phase: SelfImprovementPhaseV1::ApprovedToPlan,
        revision: 1,
    };

    let error = probe_capability_manifest_for_run_with_registry(&run, &source, &registry)
        .expect_err("the registered callback should run and return its sentinel result");
    assert_eq!(error, "keyboard-navigation probe invoked");
}
