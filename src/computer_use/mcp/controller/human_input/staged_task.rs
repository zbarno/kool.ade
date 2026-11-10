use crate::computer_use::{
    ComputerCapabilityV1, ComputerNetworkProfileV1, ComputerOperationV1, ComputerSessionClassV1,
    apps::{
        GuestAppArchitectureV1, StagedTaskApplicationBuildV1, StagedTaskApplicationV1,
        StagedTaskArtifactV1,
    },
    artifacts::{
        COMPUTER_ARTIFACT_MAX_BYTES_V1, ComputerArtifactFlowV1, ComputerTransferGrantRequestV1,
    },
    mcp::{COMPUTER_USE_RUNTIME_QA_APP_ID_V1, COMPUTER_USE_RUNTIME_QA_ROLE_V1},
};
use crate::runtime_qa::{QaComputerBuildApprovalV1, QaExecutionTargetV1, QaRunRequestV1};
use sha2::Digest as _;
use std::sync::atomic::{AtomicBool, Ordering};

use super::super::{session::unix_time_ms, types::ComputerUseMcpControllerV1};

impl ComputerUseMcpControllerV1 {
    /// Return the stable controller-signed session digest used to bind the
    /// Runtime QA target to one live guest session.
    pub fn computer_qa_session_attestation_sha256(
        &self,
        task_id: &str,
        session_id: &str,
    ) -> Result<String, &'static str> {
        let sessions = self
            .sessions
            .lock()
            .map_err(|_| "session_state_unavailable")?;
        let active = sessions.get(session_id).ok_or("session_not_active")?;
        if active.session.identity.task_id != task_id
            || active.role_id != COMPUTER_USE_RUNTIME_QA_ROLE_V1
            || active.uncertain
            || active.paused
        {
            return Err("computer_qa_session_not_authorized");
        }
        active
            .session
            .attestation_sha256()
            .map_err(|_| "computer_qa_session_attestation_invalid")
    }

    /// Stage the exact sorted build files approved for a disposable QA guest.
    /// Bytes are accepted only after the canonical build manifest digest and
    /// the persisted user approval match; P15 grants are one-use and fenced by
    /// the live Human-owned session generations.
    pub fn stage_approved_task_application(
        &self,
        session_id: &str,
        request: &QaRunRequestV1,
        approval: &QaComputerBuildApprovalV1,
        lifecycle_task_revision: u64,
        build: StagedTaskApplicationBuildV1,
        cancelled: &AtomicBool,
    ) -> Result<StagedTaskApplicationV1, &'static str> {
        let approval_sha256 =
            validate_approved_task_build(request, approval, lifecycle_task_revision, &build)?;

        let (identity, lease_generation, approval_generation, expires_at, bridge, project_id) = {
            let sessions = self
                .sessions
                .lock()
                .map_err(|_| "session_state_unavailable")?;
            let active = sessions.get(session_id).ok_or("session_not_active")?;
            if active.session.identity.session_id != session_id
                || active.session.identity.task_id != request.parent_task_uid
                || active.role_id != COMPUTER_USE_RUNTIME_QA_ROLE_V1
                || active.control_owner != super::Owner::Human
                || active.uncertain
                || active.paused
                || !active.started
                || active.session.session_class != ComputerSessionClassV1::Disposable
                || active.session.network_profile != ComputerNetworkProfileV1::Disabled
                || active.session.policy.capability != ComputerCapabilityV1::ApprovedGuiTask
                || active.session.image_digest != build.guest_image_digest
                || build.guest_architecture != GuestAppArchitectureV1::X86_64
                || !active
                    .session
                    .allowed_apps
                    .contains(COMPUTER_USE_RUNTIME_QA_APP_ID_V1)
                || !active
                    .session
                    .policy
                    .authorized_operations
                    .contains(&ComputerOperationV1::LaunchApprovedStagedTaskApp)
            {
                return Err("computer_qa_session_not_authorized");
            }
            let runtime = self.runtime.as_ref().ok_or("trusted_runtime_unavailable")?;
            (
                active.session.identity.clone(),
                active.session.lease.generation,
                active.session.approval_generation,
                active.session.lease.expires_at_unix_ms,
                runtime.artifact_bridge.clone(),
                active.session.identity.project_id.clone(),
            )
        };
        let workspace_id = uuid::Uuid::parse_str(&project_id)
            .map_err(|_| "computer_qa_workspace_identity_invalid")?;
        let qa_root = crate::runtime_qa::RuntimeQaStore::local_root_for_workspace(workspace_id)
            .map_err(|_| "computer_qa_approval_store_unavailable")?;
        let persisted_approval = crate::runtime_qa::RuntimeQaStore::new(qa_root)
            .load_computer_build_approval(request, lifecycle_task_revision)
            .map_err(|_| "computer_qa_approval_not_persisted")?;
        if persisted_approval != *approval {
            return Err("computer_qa_approval_not_current");
        }
        let now = unix_time_ms();
        let expires_at = expires_at.min(now.saturating_add(5 * 60 * 1_000));
        if expires_at <= now {
            return Err("computer_qa_session_expired");
        }
        let mut artifacts = Vec::with_capacity(build.files.len());
        for file in &build.files {
            if cancelled.load(Ordering::Acquire) {
                return Err("computer_qa_staging_cancelled");
            }
            let size = file.bytes.len() as u64;
            if size == 0 || size > COMPUTER_ARTIFACT_MAX_BYTES_V1 {
                return Err("computer_qa_artifact_size_invalid");
            }
            let digest = format!("{:x}", sha2::Sha256::digest(&file.bytes));
            let grant = bridge
                .issue_approved_task_app_grant(
                    ComputerTransferGrantRequestV1 {
                        identity: identity.clone(),
                        flow: ComputerArtifactFlowV1::TaskArtifactToGuest,
                        task_revision: approval.revision_sha.clone(),
                        relative_path: file.relative_path.clone(),
                        max_bytes: size,
                        now_unix_ms: now,
                        expires_at_unix_ms: expires_at,
                        lease_generation,
                        approval_generation,
                        user_approved: false,
                    },
                    approval_sha256.clone(),
                )
                .map_err(|_| "computer_qa_transfer_grant_failed")?;
            let receipt = bridge
                .stage_approved_task_build_bytes(
                    &grant.grant_id,
                    &identity,
                    &approval.revision_sha,
                    &file.relative_path,
                    &digest,
                    lease_generation,
                    approval_generation,
                    &approval_sha256,
                    &file.bytes,
                    now,
                )
                .map_err(|_| "computer_qa_artifact_stage_failed")?;
            artifacts.push(StagedTaskArtifactV1 {
                relative_path: file.relative_path.clone(),
                transfer_grant_id: grant.grant_id,
                receipt,
            });
        }
        let application = StagedTaskApplicationV1 {
            schema_version: 1,
            app_id: build.app_id,
            executable_id: build.executable_id,
            executable_relative_path: build.executable_relative_path,
            task_revision: approval.revision_sha.clone(),
            app_target_id: approval.app_target_id.clone(),
            execution_recipe_id: approval.execution_recipe_id.clone(),
            build_receipt_sha256: approval.build_receipt_sha256.clone(),
            build_approval_sha256: approval_sha256,
            guest_image_digest: build.guest_image_digest,
            guest_architecture: build.guest_architecture,
            artifacts,
        };
        application
            .validate_at(unix_time_ms())
            .map_err(|_| "computer_qa_staged_application_invalid")?;
        let sessions = self
            .sessions
            .lock()
            .map_err(|_| "session_state_unavailable")?;
        let active = sessions.get(session_id).ok_or("session_not_active")?;
        if active.session.identity != identity
            || active.session.lease.generation != lease_generation
            || active.session.approval_generation != approval_generation
            || active.control_owner != super::Owner::Human
            || active.uncertain
            || active.paused
        {
            return Err("computer_qa_session_changed");
        }
        Ok(application)
    }

    /// Launch a staged build only while the exact persisted User approval and
    /// the same Human-owned Runtime QA session remain current.
    pub fn launch_approved_staged_task_application(
        &self,
        session_id: &str,
        request: &QaRunRequestV1,
        approval: &QaComputerBuildApprovalV1,
        lifecycle_task_revision: u64,
        application: StagedTaskApplicationV1,
        cancelled: &AtomicBool,
    ) -> Result<crate::computer_use::guest_protocol::GuestActionOutcomeV1, &'static str> {
        let now = unix_time_ms();
        application
            .validate_at(now)
            .map_err(|_| "computer_qa_staged_application_invalid")?;
        approval
            .validate_for(
                request,
                lifecycle_task_revision,
                QaExecutionTargetV1::ComputerUseGuest,
            )
            .map_err(|_| "computer_qa_approval_invalid")?;
        let approval_sha256 = approval
            .approval_sha256()
            .map_err(|_| "computer_qa_approval_invalid")?;
        let (caller, project_id) = {
            let sessions = self
                .sessions
                .lock()
                .map_err(|_| "session_state_unavailable")?;
            let active = sessions.get(session_id).ok_or("session_not_active")?;
            validate_active_qa_session(active, request, &application.guest_image_digest)?;
            if application.app_id != COMPUTER_USE_RUNTIME_QA_APP_ID_V1
                || application.task_revision != approval.revision_sha
                || application.app_target_id != approval.app_target_id
                || application.execution_recipe_id != approval.execution_recipe_id
                || application.build_receipt_sha256 != approval.build_receipt_sha256
                || application.build_approval_sha256 != approval_sha256
                || application.guest_architecture != GuestAppArchitectureV1::X86_64
            {
                return Err("computer_qa_staged_application_stale");
            }
            (
                crate::computer_use::mcp::ComputerUseCallerV1 {
                    identity: active.session.identity.clone(),
                    actor: crate::computer_use::ComputerActionActorV1::Human,
                    role_id: active.role_id.clone(),
                },
                active.session.identity.project_id.clone(),
            )
        };
        verify_persisted_approval(&project_id, request, approval, lifecycle_task_revision)?;
        let authority = self
            .registry
            .authorize(&caller.identity, caller.actor, &caller.role_id, now)
            .map_err(|_| "human_control_not_authorized")?;
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| "session_state_unavailable")?;
        let active = sessions
            .get_mut(&caller.identity.session_id)
            .ok_or("session_not_active")?;
        validate_active_qa_session(active, request, &application.guest_image_digest)?;
        let outcome = super::super::dispatch::staged_task_action(
            self,
            active,
            authority,
            &caller,
            &caller.identity.session_id,
            application,
            cancelled,
        );
        if outcome.status != crate::computer_use::mcp::ComputerUseMcpStatusV1::Completed {
            return Err("computer_qa_staged_application_launch_failed");
        }
        outcome
            .action_outcome
            .ok_or("computer_qa_staged_application_launch_failed")
    }

    /// Consume the public Runtime QA target contract, rechecking its
    /// attestation and staged receipts against this controller's live session.
    pub fn launch_approved_computer_qa_target(
        &self,
        request: &QaRunRequestV1,
        approval: &QaComputerBuildApprovalV1,
        lifecycle_task_revision: u64,
        target: crate::runtime_qa::QaComputerTargetV1,
        cancelled: &AtomicBool,
    ) -> Result<crate::computer_use::guest_protocol::GuestActionOutcomeV1, &'static str> {
        target
            .validate_for(request)
            .map_err(|_| "computer_qa_target_invalid")?;
        approval
            .validate_for(
                request,
                lifecycle_task_revision,
                QaExecutionTargetV1::ComputerUseGuest,
            )
            .map_err(|_| "computer_qa_approval_invalid")?;
        let approval_sha256 = approval
            .approval_sha256()
            .map_err(|_| "computer_qa_approval_invalid")?;
        if target.build_approval_sha256 != approval_sha256
            || target.app_target_id != approval.app_target_id
            || target.execution_recipe_id != approval.execution_recipe_id
            || target.execution_recipe_sha256 != approval.execution_recipe_sha256
            || target.task_revision != approval.revision_sha
            || target.approved_spec_hash != approval.approved_spec_hash
            || target.application_id != COMPUTER_USE_RUNTIME_QA_APP_ID_V1
        {
            return Err("computer_qa_target_stale");
        }
        {
            let sessions = self
                .sessions
                .lock()
                .map_err(|_| "session_state_unavailable")?;
            let active = sessions
                .get(&target.identity.session_id)
                .ok_or("session_not_active")?;
            validate_active_qa_session(active, request, &target.guest_image_digest)?;
            if active.session.identity != target.identity
                || active
                    .session
                    .attestation_sha256()
                    .map_err(|_| "computer_qa_session_attestation_invalid")?
                    != target.session_attestation_sha256
            {
                return Err("computer_qa_session_attestation_stale");
            }
        }
        let guest_architecture = match target.guest_architecture.as_str() {
            "x86_64" => GuestAppArchitectureV1::X86_64,
            _ => return Err("computer_qa_architecture_unsupported"),
        };
        let application = StagedTaskApplicationV1 {
            schema_version: 1,
            app_id: target.application_id,
            executable_id: target.executable_id,
            executable_relative_path: target.relative_executable_path,
            task_revision: target.task_revision,
            app_target_id: target.app_target_id,
            execution_recipe_id: target.execution_recipe_id,
            build_receipt_sha256: request.build_artifact_sha256.clone(),
            build_approval_sha256: target.build_approval_sha256,
            guest_image_digest: target.guest_image_digest,
            guest_architecture,
            artifacts: target
                .staged_artifacts
                .into_iter()
                .map(|artifact| StagedTaskArtifactV1 {
                    relative_path: artifact.relative_path,
                    transfer_grant_id: artifact.transfer_grant_id,
                    receipt: artifact.receipt,
                })
                .collect(),
        };
        self.launch_approved_staged_task_application(
            &target.identity.session_id,
            request,
            approval,
            lifecycle_task_revision,
            application,
            cancelled,
        )
    }
}

fn validate_approved_task_build(
    request: &QaRunRequestV1,
    approval: &QaComputerBuildApprovalV1,
    lifecycle_task_revision: u64,
    build: &StagedTaskApplicationBuildV1,
) -> Result<String, &'static str> {
    approval
        .validate_for(
            request,
            lifecycle_task_revision,
            QaExecutionTargetV1::ComputerUseGuest,
        )
        .map_err(|_| "computer_qa_approval_invalid")?;
    build
        .validate()
        .map_err(|_| "computer_qa_build_manifest_invalid")?;
    if build.app_target_id != approval.app_target_id
        || build.execution_recipe_id != approval.execution_recipe_id
        || build.build_receipt_sha256 != approval.build_receipt_sha256
    {
        return Err("computer_qa_build_manifest_stale");
    }
    approval
        .approval_sha256()
        .map_err(|_| "computer_qa_approval_invalid")
}

fn validate_active_qa_session(
    active: &super::super::types::ActiveSession,
    request: &QaRunRequestV1,
    image_digest: &str,
) -> Result<(), &'static str> {
    if active.session.identity.task_id != request.parent_task_uid
        || active.role_id != COMPUTER_USE_RUNTIME_QA_ROLE_V1
        || active.control_owner != super::Owner::Human
        || active.uncertain
        || active.paused
        || !active.started
        || active.session.session_class != ComputerSessionClassV1::Disposable
        || active.session.network_profile != ComputerNetworkProfileV1::Disabled
        || active.session.policy.capability != ComputerCapabilityV1::ApprovedGuiTask
        || active.session.image_digest != image_digest
        || !active
            .session
            .allowed_apps
            .contains(COMPUTER_USE_RUNTIME_QA_APP_ID_V1)
        || !active
            .session
            .policy
            .authorized_operations
            .contains(&ComputerOperationV1::LaunchApprovedStagedTaskApp)
    {
        return Err("computer_qa_session_not_authorized");
    }
    Ok(())
}

fn verify_persisted_approval(
    project_id: &str,
    request: &QaRunRequestV1,
    approval: &QaComputerBuildApprovalV1,
    lifecycle_task_revision: u64,
) -> Result<(), &'static str> {
    let workspace_id =
        uuid::Uuid::parse_str(project_id).map_err(|_| "computer_qa_workspace_identity_invalid")?;
    let qa_root = crate::runtime_qa::RuntimeQaStore::local_root_for_workspace(workspace_id)
        .map_err(|_| "computer_qa_approval_store_unavailable")?;
    let persisted = crate::runtime_qa::RuntimeQaStore::new(qa_root)
        .load_computer_build_approval(request, lifecycle_task_revision)
        .map_err(|_| "computer_qa_approval_not_persisted")?;
    if &persisted != approval {
        return Err("computer_qa_approval_not_current");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        core::lifecycle::{GateActor, GateDecision, GateReceipt},
        runtime_qa::{QaBoundedLimitsV1, QaPlatform},
    };

    fn request() -> QaRunRequestV1 {
        let uid = || uuid::Uuid::new_v4().hyphenated().to_string();
        let file_hash = format!("{:x}", sha2::Sha256::digest(b"test executable"));
        let build_manifest = serde_json::to_vec(&[("bin/editor", file_hash)]).unwrap();
        QaRunRequestV1 {
            schema_version: 1,
            mode: Default::default(),
            runtime_image_digest: Some(format!("sha256:{}", "a".repeat(64))),
            runtime_image_profile: None,
            qa_run_id: uid(),
            parent_task_uid: uid(),
            target_id: Some("target-1".into()),
            planning_item_uid: None,
            feature_uid: uid(),
            code_head_sha: "b".repeat(64),
            build_artifact_sha256: format!("{:x}", sha2::Sha256::digest(build_manifest)),
            complete_diff_hash: "d".repeat(64),
            review_subject_sha256: "e".repeat(64),
            approved_spec_hash: "f".repeat(64),
            requirement_ids: vec!["REQ-1".into()],
            acceptance_criteria: vec!["app opens".into()],
            relevant_platforms: vec![QaPlatform::Web],
            startup_recipe_ref: "recipe-1".into(),
            startup_recipe_sha256: "1".repeat(64),
            execution_session_id: uid(),
            driver_profile_version: "computer-use/v1".into(),
            test_data_fixture_ref: None,
            dependency_cache_refs: vec![],
            bounded_limits: QaBoundedLimitsV1 {
                max_scenarios: 8,
                max_actions_per_scenario: 16,
                max_total_actions: 64,
                timeout_seconds: 120,
                max_evidence_bytes: 1024 * 1024,
                max_screenshots: 16,
            },
            author_run_id: uid(),
            author_session_id: uid(),
        }
    }

    fn approved(request: &QaRunRequestV1) -> QaComputerBuildApprovalV1 {
        let proposal_sha = "2".repeat(64);
        let scenario_sha = "3".repeat(64);
        let receipt = GateReceipt {
            receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
            subject_uid: request.parent_task_uid.clone(),
            subject_sha256: scenario_sha.clone(),
            actor: GateActor::User,
            decision: GateDecision::Approved,
            recorded_at_unix_ms: 5,
            evidence_refs: vec![
                format!("runtime-qa:{}/approved-scenarios", request.qa_run_id),
                format!(
                    "runtime-qa:{}/proposal:{}:task-revision:9",
                    request.qa_run_id, proposal_sha
                ),
                format!(
                    "runtime-qa:{}/execution-target:computer-use-guest",
                    request.qa_run_id
                ),
            ],
            prompt_sha256: None,
            prompt_manifest: None,
            reviewer_run_id: None,
            author_run_id: None,
            reviewer_session_id: None,
            author_session_id: None,
        };
        let request_sha = format!(
            "{:x}",
            sha2::Sha256::digest(serde_json::to_vec(request).unwrap())
        );
        QaComputerBuildApprovalV1 {
            schema_version: 1,
            qa_run_id: request.qa_run_id.clone(),
            task_uid: request.parent_task_uid.clone(),
            task_revision: 9,
            revision_sha: request.code_head_sha.clone(),
            approved_spec_hash: request.approved_spec_hash.clone(),
            app_target_id: request.target_id.clone().unwrap(),
            execution_recipe_id: request.startup_recipe_ref.clone(),
            execution_recipe_sha256: request.startup_recipe_sha256.clone(),
            build_receipt_sha256: request.build_artifact_sha256.clone(),
            request_sha256: request_sha,
            scenario_proposal_sha256: proposal_sha,
            scenario_sha256: scenario_sha,
            recorded_at_unix_ms: 5,
            user_approval: receipt,
        }
    }

    fn build(approval: &QaComputerBuildApprovalV1) -> StagedTaskApplicationBuildV1 {
        let bytes = b"test executable".to_vec();
        let file_hash = format!("{:x}", sha2::Sha256::digest(&bytes));
        let manifest = serde_json::to_vec(&[("bin/editor", file_hash)]).unwrap();
        StagedTaskApplicationBuildV1 {
            schema_version: 1,
            app_id: COMPUTER_USE_RUNTIME_QA_APP_ID_V1.into(),
            executable_id: "editor".into(),
            executable_relative_path: "bin/editor".into(),
            app_target_id: approval.app_target_id.clone(),
            execution_recipe_id: approval.execution_recipe_id.clone(),
            build_receipt_sha256: format!("{:x}", sha2::Sha256::digest(manifest)),
            guest_image_digest: format!("sha256:{}", "a".repeat(64)),
            guest_architecture: GuestAppArchitectureV1::X86_64,
            files: vec![crate::computer_use::apps::StagedTaskBuildFileV1 {
                relative_path: "bin/editor".into(),
                bytes,
            }],
        }
    }

    #[test]
    fn controller_build_gate_rejects_stale_revision_and_wrong_target_before_grant_creation() {
        let request = request();
        let approval = approved(&request);
        let valid_build = build(&approval);
        assert!(validate_approved_task_build(&request, &approval, 9, &valid_build).is_ok());

        assert_eq!(
            validate_approved_task_build(&request, &approval, 10, &valid_build),
            Err("computer_qa_approval_invalid")
        );
        let mut wrong_build = valid_build;
        wrong_build.app_target_id = "different-target".into();
        assert_eq!(
            validate_approved_task_build(&request, &approval, 9, &wrong_build),
            Err("computer_qa_build_manifest_stale")
        );
    }
}
