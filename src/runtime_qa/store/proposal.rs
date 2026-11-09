use super::scenarios::ApprovedQaScenarioSetV1;
use super::*;

impl RuntimeQaStore {
    /// Save a validated proposal without granting execution permission.
    pub fn save_scenario_proposal(
        &self,
        request: &QaRunRequestV1,
        proposal: &QaScenarioProposalV1,
    ) -> Result<(), String> {
        proposal.validate_for(request)?;
        let run = safe_uuid(&request.qa_run_id)?;
        let dir = self.root.join(run);
        fs::create_dir_all(&dir)
            .map_err(|error| format!("cannot create runtime QA record directory: {error}"))?;
        create_or_match_json(&self.root, &dir.join("scenario-proposal-v2.json"), proposal)
    }

    /// Reload and revalidate the persisted proposal against the frozen request.
    pub fn load_scenario_proposal(
        &self,
        request: &QaRunRequestV1,
    ) -> Result<QaScenarioProposalV1, String> {
        request.validate()?;
        let run = safe_uuid(&request.qa_run_id)?;
        let proposal: QaScenarioProposalV1 = read_json(
            &self.root,
            &self.root.join(run).join("scenario-proposal-v2.json"),
        )?;
        proposal.validate_for(request)?;
        Ok(proposal)
    }

    /// Store user approval for the exact synthesized proposal and lifecycle revision.
    pub fn approve_scenario_proposal(
        &self,
        request: &QaRunRequestV1,
        proposal: &QaScenarioProposalV1,
        task_revision: u64,
        approval: &crate::core::lifecycle::GateReceipt,
    ) -> Result<(), String> {
        self.approve_scenario_proposal_for_target(
            request,
            proposal,
            task_revision,
            super::super::QaExecutionTargetV1::IsolatedContainer,
            approval,
        )
    }

    pub fn approve_scenario_proposal_for_target(
        &self,
        request: &QaRunRequestV1,
        proposal: &QaScenarioProposalV1,
        task_revision: u64,
        execution_target: super::super::QaExecutionTargetV1,
        approval: &crate::core::lifecycle::GateReceipt,
    ) -> Result<(), String> {
        proposal.validate_for(request)?;
        let proposal_sha256 = proposal.proposal_sha256()?;
        if proposal.task_revision != task_revision {
            return Err("runtime QA approval task revision does not match its proposal".into());
        }
        if approval.subject_sha256 != proposal.scenario_sha256 {
            return Err("runtime QA approval does not bind the proposal scenario hash".into());
        }
        let expected_ref = format!(
            "runtime-qa:{}/proposal:{proposal_sha256}:task-revision:{task_revision}",
            request.qa_run_id
        );
        if !approval
            .evidence_refs
            .iter()
            .any(|reference| reference == &expected_ref)
        {
            return Err("runtime QA approval does not bind the exact proposal hash".into());
        }
        self.approve_scenarios_with_proposal(
            request,
            &proposal.scenarios,
            execution_target,
            Some(proposal_sha256),
            Some(task_revision),
            approval,
        )?;
        Ok(())
    }

    /// Reload approval only while the original proposal and lifecycle revision remain current.
    pub fn load_approved_scenario_proposal(
        &self,
        request: &QaRunRequestV1,
        task_revision: u64,
    ) -> Result<ApprovedQaScenarioSetV1, String> {
        let proposal = self.load_scenario_proposal(request)?;
        if proposal.task_revision != task_revision {
            return Err("runtime QA proposal is stale for the current task revision".into());
        }
        let approved = self.load_approved_scenarios(request)?;
        let expected = format!(
            "runtime-qa:{}/proposal:{}:task-revision:{}",
            request.qa_run_id,
            proposal.proposal_sha256()?,
            task_revision
        );
        if !approved
            .approval
            .evidence_refs
            .iter()
            .any(|reference| reference == &expected)
        {
            return Err(
                "runtime QA approval is not bound to the current synthesized proposal".into(),
            );
        }
        let current_proposal_sha256 = proposal.proposal_sha256()?;
        if (approved.proposal_sha256.is_some()
            && approved.proposal_sha256.as_deref() != Some(current_proposal_sha256.as_str()))
            || (approved.task_revision.is_some() && approved.task_revision != Some(task_revision))
            || (approved.execution_target == super::super::QaExecutionTargetV1::ComputerUseGuest
                && (approved.proposal_sha256.as_deref() != Some(current_proposal_sha256.as_str())
                    || approved.task_revision != Some(task_revision)))
        {
            return Err(
                "runtime QA approval record does not retain its exact proposal identity".into(),
            );
        }
        Ok(approved)
    }

    pub fn load_computer_build_approval(
        &self,
        request: &QaRunRequestV1,
        task_revision: u64,
    ) -> Result<crate::runtime_qa::QaComputerBuildApprovalV1, String> {
        let approved = self.load_approved_scenario_proposal(request, task_revision)?;
        let proposal_sha256 = approved
            .proposal_sha256
            .clone()
            .ok_or_else(|| "computer QA approval has no proposal identity".to_owned())?;
        let record = crate::runtime_qa::QaComputerBuildApprovalV1 {
            schema_version: 1,
            qa_run_id: request.qa_run_id.clone(),
            task_uid: request.parent_task_uid.clone(),
            task_revision,
            revision_sha: request.code_head_sha.clone(),
            approved_spec_hash: request.approved_spec_hash.clone(),
            app_target_id: request.target_id.clone().unwrap_or_default(),
            execution_recipe_id: request.startup_recipe_ref.clone(),
            execution_recipe_sha256: request.startup_recipe_sha256.clone(),
            build_receipt_sha256: request.build_artifact_sha256.clone(),
            request_sha256: approved.request_sha256.clone(),
            scenario_proposal_sha256: proposal_sha256,
            scenario_sha256: approved.scenario_sha256.clone(),
            recorded_at_unix_ms: approved.approval.recorded_at_unix_ms,
            user_approval: approved.approval,
        };
        record.validate_for(request, task_revision, approved.execution_target)?;
        Ok(record)
    }
}
