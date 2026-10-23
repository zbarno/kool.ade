use std::fs;

use super::{RuntimeQaStore, create_or_match_json, read_json, safe_uuid};
use crate::runtime_qa::{QaApplicabilityAssessmentV1, QaApplicabilityProposalV1, QaRunRequestV1};

impl RuntimeQaStore {
    pub fn load_applicability(
        &self,
        qa_run_id: &str,
    ) -> Result<QaApplicabilityAssessmentV1, String> {
        load_applicability_record(self, qa_run_id).map(|(_, assessment)| assessment)
    }

    pub fn load_applicability_for_request(
        &self,
        expected: &QaRunRequestV1,
    ) -> Result<QaApplicabilityAssessmentV1, String> {
        expected.validate()?;
        let (persisted, assessment) = load_applicability_record(self, &expected.qa_run_id)?;
        if &persisted != expected {
            return Err(
                "persisted runtime QA applicability request does not match the queued request"
                    .into(),
            );
        }
        Ok(assessment)
    }

    pub fn save_applicability_proposal(
        &self,
        proposal: &QaApplicabilityProposalV1,
    ) -> Result<String, String> {
        proposal.validate()?;
        let task = safe_uuid(&proposal.assessment.task_uid)?;
        let hash = proposal.proposal_sha256()?;
        let directory = self.root.join("applicability").join(task);
        fs::create_dir_all(&directory)
            .map_err(|error| format!("cannot create applicability proposal directory: {error}"))?;
        create_or_match_json(&directory.join(format!("{hash}.json")), proposal)?;
        Ok(hash)
    }

    pub fn load_applicability_proposal(
        &self,
        task_uid: &str,
        proposal_sha256: &str,
    ) -> Result<QaApplicabilityProposalV1, String> {
        let task = safe_uuid(task_uid)?;
        if proposal_sha256.len() != 64
            || !proposal_sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("applicability proposal hash is invalid".into());
        }
        let proposal: QaApplicabilityProposalV1 = read_json(
            &self
                .root
                .join("applicability")
                .join(task)
                .join(format!("{proposal_sha256}.json")),
        )?;
        proposal.validate()?;
        if proposal.assessment.task_uid != task_uid
            || proposal.proposal_sha256()? != proposal_sha256
        {
            return Err("applicability proposal path does not match its content".into());
        }
        Ok(proposal)
    }

    pub fn latest_applicability_proposal(
        &self,
        task_uid: &str,
        code_sha: &str,
        approved_spec_hash: &str,
    ) -> Result<Option<QaApplicabilityProposalV1>, String> {
        let task = safe_uuid(task_uid)?;
        let directory = self.root.join("applicability").join(task);
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("cannot list applicability proposals: {error}")),
        };
        let mut proposals = Vec::new();
        for entry in entries {
            let entry = entry
                .map_err(|error| format!("cannot read applicability proposal entry: {error}"))?;
            if entry
                .file_type()
                .map_err(|error| error.to_string())?
                .is_symlink()
                || !entry
                    .file_type()
                    .map_err(|error| error.to_string())?
                    .is_file()
                || entry.path().extension().and_then(|ext| ext.to_str()) != Some("json")
            {
                continue;
            }
            let proposal: QaApplicabilityProposalV1 = read_json(&entry.path())?;
            proposal.validate()?;
            if proposal.assessment.task_uid == task_uid
                && proposal.assessment.code_sha == code_sha
                && proposal.assessment.approved_spec_hash == approved_spec_hash
            {
                proposals.push(proposal);
            }
        }
        Ok(proposals
            .into_iter()
            .max_by_key(|proposal| proposal.assessment.recorded_at_unix_ms))
    }
}

fn load_applicability_record(
    store: &RuntimeQaStore,
    qa_run_id: &str,
) -> Result<(QaRunRequestV1, QaApplicabilityAssessmentV1), String> {
    let run = safe_uuid(qa_run_id)?;
    let directory = store.root.join(run);
    let request: QaRunRequestV1 = read_json(&directory.join("request.json"))?;
    request.validate()?;
    if request.qa_run_id != run {
        return Err("runtime QA record path does not match its request identity".into());
    }
    let assessment: QaApplicabilityAssessmentV1 = read_json(&directory.join("applicability.json"))?;
    assessment.validate()?;
    if assessment.task_uid != request.parent_task_uid
        || assessment.code_sha != request.code_head_sha
        || assessment.approved_spec_hash != request.approved_spec_hash
    {
        return Err("persisted runtime QA applicability assessment is stale".into());
    }
    Ok((request, assessment))
}
