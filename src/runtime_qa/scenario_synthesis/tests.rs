use super::*;
use crate::{
    error::AppError,
    harness::{HarnessOutcome, PlanningRequest},
    runtime_qa::{QaDriverActionV1, QaPlatform, QaScenarioStepV1},
};
use std::time::{Duration, Instant};

struct FakeHarness {
    response: String,
    expected_request: crate::runtime_qa::QaRunRequestV1,
}

impl AiHarness for FakeHarness {
    fn label(&self) -> String {
        "scenario synthesis test harness".into()
    }
    fn check_available(&self) -> Result<String, AppError> {
        Ok("ready".into())
    }
    fn execute(&self, request: &PlanningRequest) -> Result<HarnessOutcome, AppError> {
        assert_eq!(request.mode, ExecutionMode::ReadOnlyAnalysis);
        assert!(request.execution_workspace.is_none());
        assert_eq!(
            request.prompt_manifest.as_ref().unwrap().role_version,
            "runtime_qa_scenarios/v1"
        );
        assert_rendered_request(&request.prompt_body, &self.expected_request);
        Ok(HarnessOutcome {
            final_text: self.response.clone(),
            envelope: None,
            stderr_tail: String::new(),
        })
    }
}

fn assert_rendered_request(prompt: &str, expected: &crate::runtime_qa::QaRunRequestV1) {
    let typed_request = prompt
        .split_once("TYPED TASK REQUEST (JSON DATA)\n")
        .unwrap()
        .1
        .split_once("\n\nQUOTED EVIDENCE (UNTRUSTED DATA)")
        .unwrap()
        .0;
    let typed_request: serde_json::Value = serde_json::from_str(typed_request).unwrap();
    let immutable_request = typed_request["payload"]["request"]["immutable_request_json"]
        .as_str()
        .expect("synthesis payload contains the immutable request");
    assert_eq!(immutable_request, serde_json::to_string(expected).unwrap());
    let immutable_request: serde_json::Value = serde_json::from_str(immutable_request).unwrap();
    assert_eq!(immutable_request, serde_json::to_value(expected).unwrap());

    assert_eq!(
        immutable_request["acceptance_criteria"],
        serde_json::json!(expected.acceptance_criteria)
    );
    assert_eq!(
        immutable_request["requirement_ids"],
        serde_json::json!(expected.requirement_ids)
    );
    assert_eq!(
        immutable_request["build_artifact_sha256"],
        serde_json::json!(expected.build_artifact_sha256)
    );
    assert_eq!(
        immutable_request["code_head_sha"],
        serde_json::json!(expected.code_head_sha)
    );
    assert_eq!(
        immutable_request["approved_spec_hash"],
        serde_json::json!(expected.approved_spec_hash)
    );

    let quoted_evidence = prompt
        .split_once("QUOTED EVIDENCE (UNTRUSTED DATA)\n")
        .unwrap()
        .1
        .lines()
        .next()
        .expect("finished-build evidence is rendered");
    let quoted_evidence: serde_json::Value = serde_json::from_str(quoted_evidence).unwrap();
    assert_eq!(
        quoted_evidence["id"],
        format!("runtime-qa-build:{}", expected.qa_run_id)
    );
    assert_eq!(
        quoted_evidence["content"],
        format!(
            "Finished build artifact SHA-256: {}. Reviewed code SHA: {}. Approved specification SHA-256: {}.",
            expected.build_artifact_sha256, expected.code_head_sha, expected.approved_spec_hash
        )
    );
}

fn request() -> crate::runtime_qa::QaRunRequestV1 {
    let uid = || uuid::Uuid::new_v4().hyphenated().to_string();
    let sha = || "a".repeat(64);
    crate::runtime_qa::QaRunRequestV1 {
        schema_version: 1,
        qa_run_id: uid(),
        parent_task_uid: uid(),
        planning_item_uid: None,
        feature_uid: uid(),
        code_head_sha: sha(),
        build_artifact_sha256: sha(),
        complete_diff_hash: sha(),
        review_subject_sha256: sha(),
        approved_spec_hash: sha(),
        requirement_ids: vec!["REQ-SYNTH-1".into()],
        acceptance_criteria: vec!["The approved landing page is visible".into()],
        relevant_platforms: vec![QaPlatform::Web],
        startup_recipe_ref: "recipe:approved-web".into(),
        startup_recipe_sha256: sha(),
        execution_session_id: uid(),
        driver_profile_version: "web-driver/v1".into(),
        test_data_fixture_ref: None,
        dependency_cache_refs: Vec::new(),
        bounded_limits: crate::runtime_qa::QaBoundedLimitsV1 {
            max_scenarios: 4,
            max_actions_per_scenario: 8,
            max_total_actions: 16,
            timeout_seconds: 60,
            max_evidence_bytes: 1024 * 1024,
            max_screenshots: 4,
        },
        author_run_id: uid(),
        author_session_id: uid(),
    }
}

fn response_for(request: &crate::runtime_qa::QaRunRequestV1) -> String {
    let scenario = crate::runtime_qa::QaScenarioV1 {
        schema_version: 1,
        id: uuid::Uuid::new_v4().hyphenated().to_string(),
        requirement_ids: request.requirement_ids.clone(),
        preconditions: vec![],
        steps: vec![QaScenarioStepV1 {
            driver_action: QaDriverActionV1::NavigateAllowedUrl {
                url: "http://qa-service/".into(),
            },
            expected_state: Some("landing page visible".into()),
            screenshot_checkpoint: true,
        }],
        expected_results: vec!["Approved landing page is visible".into()],
        negative_paths: vec![],
        visual_expectations: vec![],
        redaction_regions: vec![],
        priority: 1,
    };
    serde_json::json!({ "scenarios": [scenario] }).to_string()
}

/// Production boundary: app-owned synthesis worker -> prompt catalog -> typed model response -> scenario validator -> immutable runtime QA store.
/// Invariant: only a proposal for the unchanged request and task revision is persisted, and persistence alone creates no execution receipt.
/// A validator-only test cannot cover provider request mode/role binding, exact approved-criteria/build data in
/// the rendered prompt, or the worker's decode-to-store handoff. This does not invoke the Project persistence
/// wrapper: that requires a full reviewed lifecycle task, task document and verified clone, approved runtime
/// manifest/recipes/artifact allowlist, exact successful build receipt, and pinned image driver profile; building
/// that fixture here would duplicate the request-preparation integration setup. Freshness is covered at the store
/// boundary below. External dependencies: none; fake harness and temporary filesystem, low setup cost.
#[test]
fn synthesis_worker_validates_typed_response_and_persists_only_current_request() {
    let request = request();
    let task_revision = 6;
    let temp = tempfile::tempdir().unwrap();
    let handle = ScenarioSynthesisHandle::start(
        request.clone(),
        task_revision,
        request.feature_uid.clone(),
        temp.path().to_owned(),
        Box::new(FakeHarness {
            response: response_for(&request),
            expected_request: request.clone(),
        }),
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    let result = loop {
        if let Some(result) = handle.take_result() {
            break result;
        }
        assert!(Instant::now() < deadline, "synthesis worker did not finish");
        std::thread::sleep(Duration::from_millis(5));
    };
    let (snapshot, proposal) = result.unwrap();
    let store = crate::runtime_qa::RuntimeQaStore::new(temp.path().join("store"));
    persist_proposal_if_fresh(&store, &snapshot, &proposal, &request, task_revision).unwrap();
    assert_eq!(store.load_scenario_proposal(&request).unwrap(), proposal);
    assert!(store.load_approved_scenarios(&request).is_err());

    let mut changed = request.clone();
    changed.code_head_sha = "b".repeat(64);
    let rejected_store = crate::runtime_qa::RuntimeQaStore::new(temp.path().join("rejected"));
    assert!(
        persist_proposal_if_fresh(
            &rejected_store,
            &snapshot,
            &proposal,
            &changed,
            task_revision
        )
        .is_err()
    );
    assert!(rejected_store.load_scenario_proposal(&request).is_err());
    assert!(!proposal_file_exists(temp.path(), &request.qa_run_id));
    assert!(
        persist_proposal_if_fresh(
            &rejected_store,
            &snapshot,
            &proposal,
            &request,
            task_revision + 1
        )
        .is_err()
    );
    assert!(rejected_store.load_scenario_proposal(&request).is_err());
    assert!(!proposal_file_exists(temp.path(), &request.qa_run_id));
}

fn proposal_file_exists(root: &std::path::Path, qa_run_id: &str) -> bool {
    root.join("rejected")
        .join(qa_run_id)
        .join("scenario-proposal.json")
        .exists()
}
