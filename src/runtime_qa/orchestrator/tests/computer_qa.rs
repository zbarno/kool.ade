use super::super::*;
use super::support::{fixture, reviewer_prompt};
use crate::{
    computer_use::{ComputerIdentityV1, artifacts::ComputerArtifactReceiptV1},
    runtime_qa::{
        QaComputerStagedArtifactV1, QaComputerTargetV1,
        drivers::computer_vm::{
            ComputerQaDisplayV1, ComputerQaGuestSessionV1, ComputerQaScenarioDriverV1,
        },
    },
};
use sha2::Digest as _;
use std::time::Instant;

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

const EXECUTABLE_PATH: &str = "bin/approved-app";
const EXECUTABLE_SHA: &str = "f";

fn bind_build_hash(request: &mut QaRunRequestV1) {
    let manifest = vec![(EXECUTABLE_PATH, EXECUTABLE_SHA.repeat(64))];
    request.build_artifact_sha256 = format!(
        "{:x}",
        sha2::Sha256::digest(serde_json::to_vec(&manifest).unwrap())
    );
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
        application_id: "qa-task-app".into(),
        executable_id: "main".into(),
        app_target_id: request.target_id.clone().unwrap(),
        execution_recipe_id: request.startup_recipe_ref.clone(),
        execution_recipe_sha256: request.startup_recipe_sha256.clone(),
        relative_executable_path: EXECUTABLE_PATH.into(),
        guest_image_digest: format!("sha256:{}", "1".repeat(64)),
        guest_architecture: "x86_64".into(),
        session_attestation_sha256: "2".repeat(64),
        build_approval_sha256: "3".repeat(64),
        task_revision: request.code_head_sha.clone(),
        approved_spec_hash: request.approved_spec_hash.clone(),
        display_width: 1024,
        display_height: 768,
        display_dpi: 96,
        staged_artifacts: vec![QaComputerStagedArtifactV1 {
            relative_path: EXECUTABLE_PATH.into(),
            transfer_grant_id: id(),
            receipt: ComputerArtifactReceiptV1 {
                artifact_id: id(),
                display_name: "approved-app".into(),
                size_bytes: 42,
                sha256: format!("sha256:{}", EXECUTABLE_SHA.repeat(64)),
                media_type: "application/octet-stream".into(),
                expires_at_unix_ms: 4_000_000_000_000,
            },
        }],
    }
}

#[test]
fn computer_target_is_bound_to_the_exact_task_revision_and_build() {
    let (mut request, _, _) = fixture();
    bind_build_hash(&mut request);
    let target = make_target(&request);
    target.validate_for(&request).unwrap();

    let mut stale = target.clone();
    stale.task_revision = "f".repeat(40);
    assert!(stale.validate_for(&request).is_err());

    let mut wrong_artifact = target;
    wrong_artifact.staged_artifacts[0].receipt.sha256 = format!("sha256:{}", "3".repeat(64));
    assert!(wrong_artifact.validate_for(&request).is_err());

    let mut wrong_spec = make_target(&request);
    wrong_spec.approved_spec_hash = "4".repeat(64);
    assert!(wrong_spec.validate_for(&request).is_err());

    let mut wrong_app = make_target(&request);
    wrong_app.application_id = "unapproved-app".into();
    assert!(wrong_app.validate_for(&request).is_err());
}

#[test]
fn assembled_evidence_retains_the_computer_session_and_staging_binding() {
    let (mut request, mut scenarios, _) = fixture();
    bind_build_hash(&mut request);
    request.relevant_platforms = vec![QaPlatform::LinuxDesktop];
    scenarios[0].steps[0].driver_action =
        crate::runtime_qa::QaDriverActionV1::LaunchValidatedTarget;
    let target = make_target(&request);
    let mut guest = FakeComputerGuest { launched: false };
    let mut driver = ComputerQaScenarioDriverV1 {
        request: &request,
        target: &target,
        guest: &mut guest,
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

struct FakeComputerGuest {
    launched: bool,
}

impl ComputerQaGuestSessionV1 for FakeComputerGuest {
    fn begin_scenario(
        &mut self,
        target: &QaComputerTargetV1,
        _scenario_id: &str,
        _deadline: Instant,
    ) -> Result<ComputerQaDisplayV1, String> {
        Ok(ComputerQaDisplayV1 {
            width: target.display_width,
            height: target.display_height,
            dpi: target.display_dpi,
        })
    }

    fn launch_staged_application(
        &mut self,
        target: &QaComputerTargetV1,
        _deadline: Instant,
    ) -> Result<(), String> {
        target.validate_for(&request_for_target(target))?;
        self.launched = true;
        Ok(())
    }

    fn observe_state(&mut self, _deadline: Instant) -> Result<String, String> {
        Ok(if self.launched {
            "approved app visible".into()
        } else {
            "guest ready".into()
        })
    }

    fn perform_action(
        &mut self,
        _action: &crate::runtime_qa::QaDriverActionV1,
        _deadline: Instant,
    ) -> Result<(), String> {
        Ok(())
    }

    fn screenshot_png(&mut self, _deadline: Instant) -> Result<Vec<u8>, String> {
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            1024,
            768,
            image::Rgba([20, 40, 80, 255]),
        ))
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|error| error.to_string())?;
        Ok(png.into_inner())
    }

    fn end_scenario(&mut self) -> Result<(), String> {
        self.launched = false;
        Ok(())
    }
}

fn request_for_target(target: &QaComputerTargetV1) -> QaRunRequestV1 {
    let (mut request, _, _) = fixture();
    request.qa_run_id = target.qa_run_id.clone();
    request.parent_task_uid = target.identity.task_id.clone();
    request.target_id = Some(target.app_target_id.clone());
    request.startup_recipe_ref = target.execution_recipe_id.clone();
    request.startup_recipe_sha256 = target.execution_recipe_sha256.clone();
    request.code_head_sha = target.task_revision.clone();
    request.approved_spec_hash = target.approved_spec_hash.clone();
    let manifest = target
        .staged_artifacts
        .iter()
        .map(|item| {
            (
                item.relative_path.clone(),
                item.receipt
                    .sha256
                    .strip_prefix("sha256:")
                    .unwrap()
                    .to_owned(),
            )
        })
        .collect::<Vec<_>>();
    request.build_artifact_sha256 = format!(
        "{:x}",
        sha2::Sha256::digest(serde_json::to_vec(&manifest).unwrap())
    );
    request
}

/// Existing preview drivers cannot acquire guest authority. This checks that
/// the typed Computer QA route fails before driver invocation until a
/// verified staging-to-launch adapter is installed.
#[test]
fn preview_only_driver_cannot_run_computer_qa() {
    let (mut request, mut scenarios, _) = fixture();
    bind_build_hash(&mut request);
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
