use super::super::*;
use super::support::{FakeDriver, fixture, reviewer_prompt};
use crate::computer_use::{ComputerIdentityV1, artifacts::ComputerArtifactReceiptV1};
use crate::runtime_qa::QaComputerTargetV1;

struct PreviewOnlyDesktopDriver;

impl QaScenarioDriver for PreviewOnlyDesktopDriver {
    fn platform(&self) -> QaPlatform {
        QaPlatform::LinuxDesktop
    }

    fn run_scenario(
        &mut self,
        _request: &QaRunRequestV1,
        _scenario: &QaScenarioV1,
        _authorization: &QaTargetAuthorization<'_>,
    ) -> Result<QaDriverOutputV1, String> {
        panic!("Computer QA must fail before a preview driver is invoked")
    }
}

fn make_target(request: &QaRunRequestV1) -> QaComputerTargetV1 {
    let id = || uuid::Uuid::new_v4().hyphenated().to_string();
    QaComputerTargetV1 {
        schema_version: 1,
        qa_run_id: request.qa_run_id.clone(),
        identity: ComputerIdentityV1 {
            project_id: id(),
            task_id: request.parent_task_uid.clone(),
            user_id: id(),
            session_id: id(),
        },
        application_id: "approved-app-v1".into(),
        guest_image_sha256: "1".repeat(64),
        session_attestation_sha256: "2".repeat(64),
        task_revision: request.code_head_sha.clone(),
        approved_spec_hash: request.approved_spec_hash.clone(),
        staged_artifact: ComputerArtifactReceiptV1 {
            artifact_id: id(),
            display_name: "approved-app.tar.gz".into(),
            size_bytes: 42,
            sha256: request.build_artifact_sha256.clone(),
            media_type: "application/gzip".into(),
            expires_at_unix_ms: 4_000_000_000_000,
        },
    }
}

#[test]
fn computer_target_is_bound_to_the_exact_task_revision_and_build() {
    let (request, _, _) = fixture();
    let target = make_target(&request);
    target.validate_for(&request).unwrap();

    let mut stale = target.clone();
    stale.task_revision = "f".repeat(40);
    assert!(stale.validate_for(&request).is_err());

    let mut wrong_artifact = target;
    wrong_artifact.staged_artifact.sha256 = "3".repeat(64);
    assert!(wrong_artifact.validate_for(&request).is_err());

    let mut wrong_spec = make_target(&request);
    wrong_spec.approved_spec_hash = "4".repeat(64);
    assert!(wrong_spec.validate_for(&request).is_err());
}

#[test]
fn assembled_evidence_retains_the_computer_session_and_staging_binding() {
    let (mut request, mut scenarios, _) = fixture();
    request.relevant_platforms = vec![QaPlatform::LinuxDesktop];
    scenarios[0].steps[0].driver_action =
        crate::runtime_qa::QaDriverActionV1::LaunchValidatedTarget;
    let target = make_target(&request);
    let mut driver = FakeDriver {
        calls: 0,
        assertion_status: crate::runtime_qa::QaAssertionStatus::Passed,
        failure_before_actions: false,
        failure_on_call: None,
    };
    let output = driver
        .run_scenario(
            &request,
            &scenarios[0],
            &QaTargetAuthorization::ComputerVm(&target),
        )
        .unwrap();
    let report = super::super::report::assemble_report(
        &request,
        &scenarios,
        vec![output],
        Some(target.clone()),
    )
    .unwrap();

    assert_eq!(report.evidence.computer_qa_target, Some(target));
    report.evidence.validate(&request, &scenarios).unwrap();
}

/// Existing preview drivers cannot acquire guest authority. This checks that
/// the typed Computer QA route fails before driver invocation until a
/// verified staging-to-launch adapter is installed.
#[test]
fn preview_only_driver_cannot_run_computer_qa() {
    let (mut request, mut scenarios, _) = fixture();
    request.relevant_platforms = vec![QaPlatform::LinuxDesktop];
    scenarios[0].steps[0].driver_action =
        crate::runtime_qa::QaDriverActionV1::LaunchValidatedTarget;
    let target = make_target(&request);
    let run = uuid::Uuid::new_v4().hyphenated().to_string();
    let session = uuid::Uuid::new_v4().hyphenated().to_string();
    let prompt = reviewer_prompt(&request, &run, &session);
    let root = tempfile::tempdir().unwrap();
    let store = RuntimeQaStore::new(root.path().join("qa"));
    let mut driver = PreviewOnlyDesktopDriver;

    let error = RuntimeQaOrchestrator::new(&store)
        .execute(QaExecutionContext {
            request: &request,
            scenarios: &scenarios,
            authorization: QaTargetAuthorization::ComputerVm(&target),
            driver: &mut driver,
            reviewer_execution_id: &run,
            reviewer_session_id: &session,
            prompt,
        })
        .unwrap_err();
    assert!(error.contains("no app-owned verified guest staging and launch driver"));
}
