use std::{fs, path::Path};

use crate::computer_use::ComputerActionActorV1 as Actor;
use ed25519_dalek::SigningKey;
use sha2::Digest as _;

use crate::{
    artifacts::planning_store::{PlanningStore, StoreMode},
    computer_use::{
        ComputerIdentityV1, ComputerSessionV1,
        artifacts::ComputerArtifactBridgeV1,
        guest_protocol::{
            AuthenticatedGuestChannelV1, AuthorizedComputerUseSessionV1, GuestActionJournalV1,
        },
    },
    runtime_qa::{
        QaAssertionStatus, QaComputerStagedArtifactV1, QaComputerTargetV1, QaDriverActionV1,
        QaPlatform, QaRunRequestV1, QaScenarioDriver, QaScenarioStepV1, QaTargetAuthorization,
        drivers::computer_vm::ComputerQaScenarioDriverV1,
    },
};

pub(super) const TASK_APP_ID: &str = crate::computer_use::mcp::COMPUTER_USE_RUNTIME_QA_APP_ID_V1;
pub(super) const EXECUTABLE_PATH: &str = "bin/qa-fixture";

/// Production boundary: the production Computer QA scenario driver stages an
/// approved native app through the real artifact bridge, launches it in the
/// signed QEMU guest, and records either a failed seeded assertion or a passing
/// corrected build with authenticated guest input and screenshot evidence.
/// Component fixtures cannot establish that the guest helper launched the
/// transferred ELF or that real guest observations change after input.
/// Dependencies are the trusted KVM/image/cgroup prerequisites and the seeded
/// fixture binaries prebuilt by the opt-in workflow; no network is enabled.
/// Each build runs in its own fresh disposable VM session because the guest
/// staging protocol allows one app launch per session.
#[allow(clippy::too_many_arguments)]
pub(super) fn run_seeded_build(
    root: &Path,
    session: &ComputerSessionV1,
    controller_key_id: &str,
    controller_key: &SigningKey,
    authority: &AuthorizedComputerUseSessionV1,
    channel: &mut AuthenticatedGuestChannelV1,
    journal: &GuestActionJournalV1,
    first_sequence: u64,
) -> Result<u64, String> {
    let (build_label, fixture_variable, expected_status) =
        match std::env::var("KOOLADE_R1_QA_BUILD").as_deref() {
            Ok("defect") => (
                "seeded-defect",
                "KOOLADE_R1_QA_DEFECT_FIXTURE",
                QaAssertionStatus::Failed,
            ),
            Ok("corrected") => (
                "corrected-build",
                "KOOLADE_R1_QA_CORRECTED_FIXTURE",
                QaAssertionStatus::Passed,
            ),
            _ => return Err("KOOLADE_R1_QA_BUILD must be defect or corrected".into()),
        };
    let fixture = fixture_path(fixture_variable)?;
    let store = PlanningStore::new(
        uuid::Uuid::new_v4(),
        root.join("real-qa-artifact-store"),
        StoreMode::ManagedLocal,
    );
    let bridge = ComputerArtifactBridgeV1::new(store);
    let mut guest = LiveQaGuest {
        session,
        actor: Actor::Human,
        controller_key_id,
        controller_key,
        authority,
        channel,
        journal,
        bridge: &bridge,
        next_sequence: first_sequence,
        fixture: fixture.clone(),
        request: None,
        preexisting_windows: Default::default(),
        latest: None,
    };

    let output = run_one_build(&mut guest, session, build_label, &fixture)?;
    if !output.completed
        || output.assertions.len() != 2
        || output.assertions[0].status != QaAssertionStatus::Passed
        || output.assertions[1].status != expected_status
        || output.screenshots.len() != 1
    {
        return Err(format!(
            "real VM QA {build_label} evidence mismatch: completed={} failure={:?} assertions={:?} screenshots={}",
            output.completed,
            output.failure,
            output
                .assertions
                .iter()
                .map(|assertion| (&assertion.status, &assertion.observed))
                .collect::<Vec<_>>(),
            output.screenshots.len(),
        ));
    }
    println!(
        "R1 real VM QA evidence: build={} assertion={:?} screenshot_sha256={}",
        build_label, output.assertions[1].status, output.screenshots[0].sha256,
    );
    Ok(guest.next_sequence)
}

fn run_one_build(
    guest: &mut LiveQaGuest<'_>,
    session: &ComputerSessionV1,
    build_label: &str,
    fixture: &Path,
) -> Result<crate::runtime_qa::QaDriverOutputV1, String> {
    let (mut request, mut scenarios, _) =
        crate::runtime_qa::orchestrator::tests::support::fixture();
    request.relevant_platforms = vec![QaPlatform::LinuxDesktop];
    request.parent_task_uid = session.identity.task_id.clone();
    request.driver_profile_version = "computer-vm-driver/v1".into();
    let bytes = fs::read(fixture)
        .map_err(|error| format!("could not read prebuilt {build_label} QA fixture: {error}"))?;
    let file_sha = format!("{:x}", sha2::Sha256::digest(&bytes));
    let manifest = vec![(EXECUTABLE_PATH, file_sha.as_str())];
    request.build_artifact_sha256 = format!(
        "{:x}",
        sha2::Sha256::digest(
            serde_json::to_vec(&manifest)
                .map_err(|error| format!("encode {build_label} build manifest: {error}"))?
        )
    );
    let target = target_for(&request, session, &file_sha, bytes.len() as u64)?;
    let scenario = &mut scenarios[0];
    scenario.steps = vec![
        QaScenarioStepV1 {
            driver_action: QaDriverActionV1::LaunchValidatedTarget,
            expected_state: None,
            screenshot_checkpoint: false,
        },
        QaScenarioStepV1 {
            driver_action: QaDriverActionV1::WaitForWindow,
            expected_state: Some("computer qa destination".into()),
            screenshot_checkpoint: false,
        },
        QaScenarioStepV1 {
            driver_action: QaDriverActionV1::Click {
                window_id: "active".into(),
                x: 60,
                y: 70,
            },
            expected_state: Some("destination /expected".into()),
            screenshot_checkpoint: true,
        },
    ];
    scenario.expected_results = vec!["destination /expected".into()];
    let authorization = QaTargetAuthorization::ComputerVm(&target);
    guest.request = Some(request.clone());
    let mut driver = ComputerQaScenarioDriverV1 {
        request: &request,
        target: &target,
        guest,
    };
    driver
        .run_scenario(&request, scenario, &authorization)
        .map_err(|error| format!("real VM {build_label} QA scenario: {error}"))
}

fn fixture_path(variable: &str) -> Result<std::path::PathBuf, String> {
    let path = std::env::var_os(variable)
        .map(std::path::PathBuf::from)
        .ok_or_else(|| format!("required prebuilt real-VM QA fixture path is unset: {variable}"))?;
    if !path.is_file() || path.is_symlink() {
        return Err(format!(
            "real-VM QA fixture is not a regular file: {variable}"
        ));
    }
    Ok(path)
}

fn target_for(
    request: &QaRunRequestV1,
    session: &ComputerSessionV1,
    file_sha: &str,
    size_bytes: u64,
) -> Result<QaComputerTargetV1, String> {
    let id = || uuid::Uuid::new_v4().hyphenated().to_string();
    Ok(QaComputerTargetV1 {
        schema_version: 1,
        qa_run_id: request.qa_run_id.clone(),
        identity: ComputerIdentityV1 {
            project_id: session.identity.project_id.clone(),
            task_id: request.parent_task_uid.clone(),
            user_id: session.identity.user_id.clone(),
            session_id: session.identity.session_id.clone(),
        },
        application_id: TASK_APP_ID.into(),
        executable_id: "qa-fixture".into(),
        app_target_id: request.target_id.clone().unwrap_or_default(),
        execution_recipe_id: request.startup_recipe_ref.clone(),
        execution_recipe_sha256: request.startup_recipe_sha256.clone(),
        relative_executable_path: EXECUTABLE_PATH.into(),
        guest_image_digest: session.image_digest.clone(),
        guest_architecture: "x86_64".into(),
        session_attestation_sha256: "2".repeat(64),
        build_approval_sha256: "3".repeat(64),
        task_revision: request.code_head_sha.clone(),
        approved_spec_hash: request.approved_spec_hash.clone(),
        display_width: 1280,
        display_height: 800,
        display_dpi: 96,
        staged_artifacts: vec![QaComputerStagedArtifactV1 {
            relative_path: EXECUTABLE_PATH.into(),
            transfer_grant_id: id(),
            receipt: crate::computer_use::artifacts::ComputerArtifactReceiptV1 {
                artifact_id: id(),
                display_name: "qa-fixture".into(),
                size_bytes,
                sha256: format!("sha256:{file_sha}"),
                media_type: "application/x-elf".into(),
                expires_at_unix_ms: 4_000_000_000_000,
            },
        }],
    })
}

#[path = "qa_driver/guest.rs"]
mod guest;
#[path = "qa_driver/staging.rs"]
mod staging;
use guest::LiveQaGuest;
