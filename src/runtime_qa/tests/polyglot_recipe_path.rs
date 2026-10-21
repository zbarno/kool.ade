//! Contract bridge from stack-neutral discovery and approved recipes into QA.
//! The app process itself is intentionally exercised only by Docker E2E; this
//! deterministic test verifies the typed handoff and evidence binding.
use super::*;
use crate::{
    project_runtime::{
        AppSurface, DiscoveryContext, ReadinessProbeV1, RuntimeOperation, RuntimeReceiptState,
        TargetCapabilityState, apply_user_recipe_override, discover_manifest,
    },
    project_runtime_capability::{ApprovedCapabilityInventory, assess_manifest_target},
};
use sha2::Digest;
use std::collections::BTreeSet;

mod support;
use support::*;

#[test]
fn approved_python_recipe_receipts_select_web_qa_and_persist_hash_bound_evidence() {
    let fixture_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/runtime_qa/python_web");
    let context = DiscoveryContext {
        workspace_id: "workspace-polyglot".into(),
        repository_id: "repo-polyglot".into(),
        owning_task_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        source_sha: "a".repeat(64),
    };
    let mut manifest = discover_manifest(&fixture_root, &context).unwrap();
    assert!(
        manifest
            .detected_languages
            .iter()
            .any(|language| language == "Python")
    );
    assert!(
        manifest
            .candidate_targets
            .iter()
            .any(|target| target.app_surface == AppSurface::Web)
    );
    let target_index = manifest
        .candidate_targets
        .iter()
        .position(|target| target.app_surface == AppSurface::Web)
        .unwrap();
    let target_id = manifest.candidate_targets[target_index].target_id.clone();
    let approval = "user-approval:synthetic-python-fixture";

    for (operation, recipe) in [
        (
            RuntimeOperation::Build,
            approved_recipe("py-build", "python", approval, &["build/app.bin"]),
        ),
        (
            RuntimeOperation::Test,
            approved_recipe("py-test", "python", approval, &["test/result.json"]),
        ),
        (
            RuntimeOperation::Start,
            approved_recipe("py-start", "python", approval, &["run/ready.json"]),
        ),
    ] {
        apply_user_recipe_override(&mut manifest, &target_id, operation, recipe).unwrap();
    }
    manifest.readiness_probes.insert(
        "py-ready".into(),
        ReadinessProbeV1::HttpOnApprovedPort {
            port: 4173,
            path: "/health".into(),
            expected_status: 200,
        },
    );
    let target = manifest
        .candidate_targets
        .iter_mut()
        .find(|target| target.target_id == target_id)
        .unwrap();
    target.readiness_probe_ref = Some("py-ready".into());
    target.supported_driver_ids = vec!["web-driver".into()];
    // Recipe observations are modeled as Docker receipts; this test never
    // executes project-provided code on the test host.
    let tool_ids = ["python".to_owned()].into_iter().collect::<BTreeSet<_>>();
    let driver_ids = ["web-driver".to_owned()]
        .into_iter()
        .collect::<BTreeSet<_>>();
    let inventory = ApprovedCapabilityInventory {
        manifest: &manifest,
        available_tool_ids: &tool_ids,
        available_dependency_ids: &BTreeSet::new(),
        available_driver_ids: &driver_ids,
    };
    let assessment =
        assess_manifest_target(&manifest, &target_id, &context.owning_task_id, &inventory).unwrap();
    assert!(
        assessment.report.gaps.is_empty(),
        "{:?}",
        assessment.report.gaps
    );
    assert_eq!(assessment.capability_state, TargetCapabilityState::Ready);
    assert_eq!(
        manifest.candidate_targets[target_index].app_surface,
        AppSurface::Web
    );
    assert!(
        !manifest
            .user_overrides
            .iter()
            .any(|recipe| recipe.executable_id == "cargo")
    );

    let build_hash = format!(
        "{:x}",
        sha2::Sha256::digest(b"synthetic python build artifact")
    );
    let observed_receipts = [
        receipt(
            &context,
            &target_id,
            RuntimeOperation::Build,
            "py-build",
            "build/app.bin",
            &build_hash,
        ),
        receipt(
            &context,
            &target_id,
            RuntimeOperation::Test,
            "py-test",
            "test/result.json",
            &"1".repeat(64),
        ),
        receipt(
            &context,
            &target_id,
            RuntimeOperation::Start,
            "py-start",
            "run/ready.json",
            &"2".repeat(64),
        ),
    ];
    for expected in ["py-build", "py-test", "py-start"] {
        let observed = observed_receipts
            .iter()
            .find(|receipt| receipt.recipe_id.as_deref() == Some(expected))
            .unwrap();
        observed.validate().unwrap();
        assert_eq!(observed.state, RuntimeReceiptState::Passed);
        assert_eq!(observed.source_sha, context.source_sha);
    }
    assert_eq!(
        observed_receipts[0].artifact_hashes["build/app.bin"],
        build_hash
    );
    let ReadinessProbeV1::HttpOnApprovedPort {
        port: 4173,
        expected_status: 200,
        path,
    } = manifest.readiness_probes.get("py-ready").unwrap()
    else {
        panic!("selected web target must carry its approved HTTP readiness probe");
    };
    assert_eq!(path, "/health");
    let start_recipe = manifest
        .user_overrides
        .iter()
        .find(|recipe| recipe.recipe_id == "py-start")
        .unwrap();
    let start_recipe_hash = format!(
        "{:x}",
        sha2::Sha256::digest(serde_json::to_vec(start_recipe).unwrap())
    );
    let request = qa_request(&context, &build_hash, "py-start", &start_recipe_hash);
    let scenario = QaScenarioV1 {
        schema_version: 1,
        id: uuid::Uuid::new_v4().hyphenated().to_string(),
        requirement_ids: request.requirement_ids.clone(),
        preconditions: vec!["Python target build, tests, and HTTP readiness passed".into()],
        steps: vec![QaScenarioStepV1 {
            driver_action: QaDriverActionV1::NavigateAllowedUrl {
                url: "http://qa-service/".into(),
            },
            expected_state: Some("fixture page visible".into()),
            screenshot_checkpoint: true,
        }],
        expected_results: vec!["fixture page visible".into()],
        negative_paths: vec![],
        visual_expectations: vec![],
        redaction_regions: vec![],
        priority: 1,
    };
    let lease = QaTaskServiceLeaseV1 {
        schema_version: 1,
        qa_run_id: request.qa_run_id.clone(),
        task_uid: request.parent_task_uid.clone(),
        task_container_id: "b".repeat(64),
        task_network_id: "c".repeat(64),
        preview_port: 4173,
        task_build_sha: build_hash.clone(),
        approved_spec_hash: request.approved_spec_hash.clone(),
        startup_recipe_ref: "py-start".into(),
        startup_recipe_sha256: start_recipe_hash,
        internal_network_only: true,
        published_ports: false,
        expires_at_unix_ms: unix_ms() + 60_000,
    };
    lease.validate_for(&request).unwrap();

    let reviewer_execution_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let reviewer_session_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let prompt = reviewer_prompt(&request, &reviewer_execution_id, &reviewer_session_id);
    let directory = tempfile::tempdir().unwrap();
    let store = RuntimeQaStore::new(directory.path());
    let mut reviewer = |evidence: &QaEvidenceManifestV1, _candidate: &QaResultV1| {
        use crate::harness::prompt_catalog::{CoverageStatus, CoverageV1, ReviewVerdictV1};
        Ok(ReviewVerdictV1 {
            subject_hash: evidence.sha256()?,
            examined_files: Vec::new(),
            tested_evidence_ids: Vec::new(),
            unexamined_areas: Vec::new(),
            pass: true,
            outcome: None,
            coverage: vec![CoverageV1 {
                requirement: request.requirement_ids[0].clone(),
                status: CoverageStatus::Pass,
                evidence: vec![evidence.actions[0].index.to_string()],
            }],
            findings: vec![],
            untested: vec![],
        })
    };
    let (result, _) = RuntimeQaOrchestrator::new(&store)
        .execute_reviewed(
            QaExecutionContext {
                request: &request,
                scenarios: &[scenario],
                authorization: QaTargetAuthorization::Web(&lease),
                driver: &mut ObservedWebDriver,
                reviewer_execution_id: &reviewer_execution_id,
                reviewer_session_id: &reviewer_session_id,
                prompt,
            },
            &mut reviewer,
        )
        .unwrap();
    assert_eq!(result.outcome, QaOutcome::Pass);
    assert_eq!(result.subject_sha, context.source_sha);
    let stored = store.load_run(&request.qa_run_id).unwrap();
    assert_eq!(stored.2.code_sha, context.source_sha);
    assert_eq!(stored.2.build_hash, build_hash);
    assert_eq!(
        stored.3.evidence_manifest_sha256,
        stored.2.sha256().unwrap()
    );
    assert!(
        stored
            .2
            .screenshots
            .iter()
            .all(|shot| shot.storage_ref.starts_with("qa-evidence/"))
    );
}
