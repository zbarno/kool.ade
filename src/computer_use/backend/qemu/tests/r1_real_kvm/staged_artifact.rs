use crate::{
    artifacts::planning_store::{PlanningStore, StoreMode},
    computer_use::{
        ComputerActionActorV1 as Actor, ComputerActionHelperPhaseV1 as HelperPhase,
        ComputerActionPayloadV1 as Payload, ComputerOperationV1 as Operation, ComputerSessionV1,
        apps::{GuestAppArchitectureV1, StagedTaskApplicationV1, StagedTaskArtifactV1},
        artifacts::{
            ComputerArtifactBridgeV1, ComputerArtifactFlowV1, ComputerTransferGrantRequestV1,
        },
        guest_protocol::{
            AuthenticatedGuestChannelV1, GuestActionJournalV1, GuestOperationResultV1,
        },
    },
};
use ed25519_dalek::SigningKey;
use sha2::Digest as _;

/// Send a synthetic QA artifact through the real P15 bridge and authenticated
/// guest channel, then launch it through the guest helper. The tiny ELF fixture
/// validates the transfer and launch protocol only; it does not prove app
/// readiness or a real QA approval.
pub(super) fn stage_artifact_through_real_guest(
    root: &std::path::Path,
    session: &ComputerSessionV1,
    controller_key_id: &str,
    controller_key: &SigningKey,
    authority: &crate::computer_use::guest_protocol::AuthorizedComputerUseSessionV1,
    channel: &mut AuthenticatedGuestChannelV1,
    journal: &GuestActionJournalV1,
) -> Result<(), String> {
    let now = super::unix_time_ms()?;
    let task_revision = "a".repeat(40);
    let approval_sha256 = "c".repeat(64);
    let relative_path = "bin/fixture";
    let mut elf_fixture = vec![0_u8; 20];
    elf_fixture[..4].copy_from_slice(b"\x7fELF");
    elf_fixture[4] = 2;
    elf_fixture[5] = 1;
    elf_fixture[16..18].copy_from_slice(&2_u16.to_le_bytes());
    elf_fixture[18..20].copy_from_slice(&62_u16.to_le_bytes());
    let file_digest = format!("{:x}", sha2::Sha256::digest(&elf_fixture));
    let artifact_manifest = serde_json::to_vec(&[(relative_path, file_digest.as_str())])
        .map_err(|error| format!("encode staged artifact manifest: {error}"))?;
    let build_receipt_sha256 = format!("{:x}", sha2::Sha256::digest(artifact_manifest));

    let planning = PlanningStore::new(
        uuid::Uuid::new_v4(),
        root.join("planning-store"),
        StoreMode::ManagedLocal,
    );
    let bridge = ComputerArtifactBridgeV1::new(planning);
    let grant = bridge
        .issue_approved_task_app_grant(
            ComputerTransferGrantRequestV1 {
                identity: session.identity.clone(),
                flow: ComputerArtifactFlowV1::TaskArtifactToGuest,
                task_revision: task_revision.clone(),
                relative_path: relative_path.into(),
                max_bytes: elf_fixture.len() as u64,
                now_unix_ms: now,
                expires_at_unix_ms: now.saturating_add(60_000),
                lease_generation: session.lease.generation,
                approval_generation: session.approval_generation,
                user_approved: false,
            },
            approval_sha256.clone(),
        )
        .map_err(|error| format!("issue synthetic staged-app grant: {error:?}"))?;
    let receipt = bridge
        .stage_approved_task_build_bytes(
            &grant.grant_id,
            &session.identity,
            &task_revision,
            relative_path,
            &format!("sha256:{file_digest}"),
            session.lease.generation,
            session.approval_generation,
            &approval_sha256,
            &elf_fixture,
            now,
        )
        .map_err(|error| format!("stage synthetic artifact: {error:?}"))?;
    let application = StagedTaskApplicationV1 {
        schema_version: 1,
        app_id: crate::computer_use::mcp::COMPUTER_USE_RUNTIME_QA_APP_ID_V1.into(),
        executable_id: "fixture".into(),
        executable_relative_path: relative_path.into(),
        task_revision: task_revision.clone(),
        app_target_id: "fixture-target".into(),
        execution_recipe_id: "fixture-recipe".into(),
        build_receipt_sha256,
        build_approval_sha256: approval_sha256,
        guest_image_digest: session.image_digest.clone(),
        guest_architecture: GuestAppArchitectureV1::X86_64,
        artifacts: vec![StagedTaskArtifactV1 {
            relative_path: relative_path.into(),
            transfer_grant_id: grant.grant_id,
            receipt,
        }],
    };
    application
        .validate_at(now)
        .map_err(|error| format!("validate staged application fixture: {error}"))?;
    let payload = Payload::LaunchApprovedStagedTaskApp {
        application: Box::new(application),
    };
    let mut stage_action = super::action_as(
        session,
        Actor::Human,
        11,
        Operation::LaunchApprovedStagedTaskApp,
        payload,
    );
    stage_action
        .sign_for_guest_helper(controller_key_id, HelperPhase::Stage, controller_key)
        .map_err(|error| format!("sign staged-artifact phase: {error}"))?;
    let mut launch_action = stage_action.clone();
    launch_action
        .sign_for_guest_helper(controller_key_id, HelperPhase::Launch, controller_key)
        .map_err(|error| format!("sign staged-app launch phase: {error}"))?;

    let result = authority
        .execute_staged_task_application(
            channel,
            journal,
            &bridge,
            &launch_action,
            &stage_action,
            &task_revision,
            now.saturating_add(60_000),
            now,
        )
        .map_err(|error| {
            let session_root = root
                .join("runtime/sessions")
                .join(&session.identity.session_id);
            let serial = std::fs::read_to_string(session_root.join("guest-serial.log"))
                .unwrap_or_default();
            let qemu_stderr = std::fs::read_to_string(session_root.join("qemu-stderr.log"))
                .unwrap_or_default();
            format!(
                "authenticated guest artifact transfer: {error:?}; guest serial: {}; QEMU stderr: {}",
                serial.chars().take(4_000).collect::<String>(),
                qemu_stderr.chars().take(2_000).collect::<String>()
            )
        })?;
    if result.reply.result != GuestOperationResultV1::Completed {
        return Err(format!(
            "staged ELF transfer or launch did not complete (result={:?})",
            result.reply.result
        ));
    }
    Ok(())
}
