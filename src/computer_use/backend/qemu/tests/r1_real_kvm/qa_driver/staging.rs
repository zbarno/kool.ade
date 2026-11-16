use super::guest::LiveQaGuest;
use super::{EXECUTABLE_PATH, TASK_APP_ID};
use crate::computer_use::{
    ComputerActionPayloadV1 as Payload, ComputerOperationV1 as Operation,
    apps::{GuestAppArchitectureV1, StagedTaskApplicationV1, StagedTaskArtifactV1},
    artifacts::{ComputerArtifactFlowV1, ComputerTransferGrantRequestV1},
    guest_protocol::GuestOperationResultV1,
};
use crate::runtime_qa::QaComputerTargetV1;
use sha2::Digest as _;
use std::fs;

impl LiveQaGuest<'_> {
    pub(super) fn stage_and_launch(&mut self, target: &QaComputerTargetV1) -> Result<(), String> {
        let request = self
            .request
            .as_ref()
            .ok_or("real QA request was not bound")?;
        target.validate_for(request)?;
        let bytes = fs::read(&self.fixture).map_err(|error| error.to_string())?;
        let digest = format!("{:x}", sha2::Sha256::digest(&bytes));
        let now = super::super::unix_time_ms()?;
        let approval_sha = target.build_approval_sha256.clone();
        let grant = self
            .bridge
            .issue_approved_task_app_grant(
                ComputerTransferGrantRequestV1 {
                    identity: self.session.identity.clone(),
                    flow: ComputerArtifactFlowV1::TaskArtifactToGuest,
                    task_revision: target.task_revision.clone(),
                    relative_path: EXECUTABLE_PATH.into(),
                    max_bytes: bytes.len() as u64,
                    now_unix_ms: now,
                    expires_at_unix_ms: now.saturating_add(60_000),
                    lease_generation: self.session.lease.generation,
                    approval_generation: self.session.approval_generation,
                    user_approved: false,
                },
                approval_sha.clone(),
            )
            .map_err(|error| format!("real QA artifact grant: {error:?}"))?;
        let receipt = self
            .bridge
            .stage_approved_task_build_bytes(
                &grant.grant_id,
                &self.session.identity,
                &target.task_revision,
                EXECUTABLE_PATH,
                &format!("sha256:{digest}"),
                self.session.lease.generation,
                self.session.approval_generation,
                &approval_sha,
                &bytes,
                now,
            )
            .map_err(|error| format!("real QA artifact staging: {error:?}"))?;
        let application = StagedTaskApplicationV1 {
            schema_version: 1,
            app_id: TASK_APP_ID.into(),
            executable_id: target.executable_id.clone(),
            executable_relative_path: EXECUTABLE_PATH.into(),
            task_revision: target.task_revision.clone(),
            app_target_id: target.app_target_id.clone(),
            execution_recipe_id: target.execution_recipe_id.clone(),
            build_receipt_sha256: self
                .request
                .as_ref()
                .ok_or("real QA request was not bound")?
                .build_artifact_sha256
                .clone(),
            build_approval_sha256: approval_sha,
            guest_image_digest: self.session.image_digest.clone(),
            guest_architecture: GuestAppArchitectureV1::X86_64,
            artifacts: vec![StagedTaskArtifactV1 {
                relative_path: EXECUTABLE_PATH.into(),
                transfer_grant_id: grant.grant_id,
                receipt,
            }],
        };
        application
            .validate_at(now)
            .map_err(|error| format!("real QA staged-app validation: {error:?}"))?;
        let mut stage = super::super::action_as(
            self.session,
            self.actor,
            self.next_sequence,
            Operation::LaunchApprovedStagedTaskApp,
            Payload::LaunchApprovedStagedTaskApp {
                application: Box::new(application),
            },
        );
        stage
            .sign_for_guest_helper(
                self.controller_key_id,
                crate::computer_use::ComputerActionHelperPhaseV1::Stage,
                self.controller_key,
            )
            .map_err(|error| format!("real QA stage signature: {error}"))?;
        let mut launch = stage.clone();
        launch
            .sign_for_guest_helper(
                self.controller_key_id,
                crate::computer_use::ComputerActionHelperPhaseV1::Launch,
                self.controller_key,
            )
            .map_err(|error| format!("real QA launch signature: {error}"))?;
        let result = self
            .authority
            .execute_staged_task_application(
                self.channel,
                self.journal,
                self.bridge,
                &launch,
                &stage,
                &target.task_revision,
                now.saturating_add(60_000),
                now,
            )
            .map_err(|error| format!("real QA app launch through guest helper: {error:?}"))?;
        self.next_sequence += 1;
        if result.reply.result != GuestOperationResultV1::Completed {
            return Err(format!(
                "real QA staged-app transfer or launch was denied: {:?}",
                result.reply.result
            ));
        }
        Ok(())
    }
}
