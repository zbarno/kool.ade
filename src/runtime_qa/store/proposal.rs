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
        create_or_match_json(&dir.join("scenario-proposal-v2.json"), proposal)
    }

    /// Reload and revalidate the persisted proposal against the frozen request.
    pub fn load_scenario_proposal(
        &self,
        request: &QaRunRequestV1,
    ) -> Result<QaScenarioProposalV1, String> {
        request.validate()?;
        let run = safe_uuid(&request.qa_run_id)?;
        let proposal: QaScenarioProposalV1 =
            read_json(&self.root.join(run).join("scenario-proposal-v2.json"))?;
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
        self.approve_scenarios(request, &proposal.scenarios, approval)?;
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
        Ok(approved)
    }
}
