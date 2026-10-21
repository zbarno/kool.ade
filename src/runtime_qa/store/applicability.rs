use std::fs;

use super::{RuntimeQaStore, create_or_match_json, read_json, safe_uuid};
use crate::runtime_qa::QaApplicabilityProposalV1;

impl RuntimeQaStore {
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
