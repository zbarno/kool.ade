use std::{
    fs,
    path::{Path, PathBuf},
};

mod applicability;
mod browser_binding;
mod gate;
mod proposal;
mod scenarios;
mod screenshots;
mod storage_root;

use super::{
    QaEvidenceManifestV1, QaFindingDispositionV1, QaResultV1, QaRunRequestV1, QaScenarioProposalV1,
    QaScenarioV1, QaScreenshotArtifactV1, validate_report,
};

/// Append-only filesystem store for runtime QA run records.
#[derive(Clone, Debug)]
pub struct RuntimeQaStore {
    pub(super) root: PathBuf,
}

/// A validated QA run together with its locally stored screenshot bytes.
pub struct QaPersistedRunV1 {
    pub request: QaRunRequestV1,
    pub scenarios: Vec<QaScenarioV1>,
    pub evidence: QaEvidenceManifestV1,
    pub result: QaResultV1,
    pub screenshots: Vec<QaScreenshotArtifactV1>,
}

impl RuntimeQaStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Return the persistent per-workspace evidence directory in the current
    /// user's application data area. Runtime QA screenshots and logs must not
    /// be written into a project checkout or its planning Git repository.
    pub fn local_root_for_workspace(workspace_id: uuid::Uuid) -> Result<PathBuf, String> {
        storage_root::local_root_for_workspace(workspace_id)
    }

    pub fn save_run(
        &self,
        request: &QaRunRequestV1,
        scenarios: &[QaScenarioV1],
        evidence: &QaEvidenceManifestV1,
        result: &QaResultV1,
    ) -> Result<(), String> {
        validate_report(request, scenarios, evidence, result)?;
        if !evidence.screenshots.is_empty() {
            return Err("runtime QA screenshots must be persisted with their image bytes".into());
        }
        self.save_run_records(request, scenarios, evidence, result)
    }

    pub fn save_run_with_screenshots(
        &self,
        request: &QaRunRequestV1,
        scenarios: &[QaScenarioV1],
        evidence: &QaEvidenceManifestV1,
        result: &QaResultV1,
        screenshots: &[QaScreenshotArtifactV1],
    ) -> Result<(), String> {
        validate_report(request, scenarios, evidence, result)?;
        screenshots::validate_screenshot_set(request, evidence, screenshots)?;
        for metadata in &evidence.screenshots {
            let artifact = screenshots
                .iter()
                .find(|artifact| artifact.id == metadata.id)
                .ok_or_else(|| "runtime QA screenshot bytes are missing".to_owned())?;
            self.write_screenshot(&metadata.storage_ref, &artifact.bytes)?;
        }
        self.save_run_records(request, scenarios, evidence, result)
    }

    fn save_run_records(
        &self,
        request: &QaRunRequestV1,
        scenarios: &[QaScenarioV1],
        evidence: &QaEvidenceManifestV1,
        result: &QaResultV1,
    ) -> Result<(), String> {
        let run = safe_uuid(&request.qa_run_id)?;
        let dir = self.root.join(run);
        fs::create_dir_all(&dir)
            .map_err(|error| format!("cannot create runtime QA record directory: {error}"))?;
        create_or_match_json(&dir.join("request.json"), request)?;
        create_json(&dir.join("scenarios.json"), scenarios)?;
        create_json(&dir.join("evidence.json"), evidence)?;
        create_json(&dir.join("result.json"), result)
    }

    /// Load and revalidate a complete persisted run. A directory left behind
    /// by an interrupted save is deliberately not treated as a completed run.
    pub fn load_run(
        &self,
        qa_run_id: &str,
    ) -> Result<
        (
            QaRunRequestV1,
            Vec<QaScenarioV1>,
            QaEvidenceManifestV1,
            QaResultV1,
        ),
        String,
    > {
        let run = self.load_run_with_screenshots(qa_run_id)?;
        Ok((run.request, run.scenarios, run.evidence, run.result))
    }

    /// Persist an explicit applicability decision alongside the exact run
    /// subject. This is evidence for Manager review, not a QA pass receipt.
    pub fn save_applicability(
        &self,
        request: &QaRunRequestV1,
        assessment: &super::QaApplicabilityAssessmentV1,
    ) -> Result<(), String> {
        request.validate()?;
        assessment.validate()?;
        if assessment.task_uid != request.parent_task_uid
            || assessment.code_sha != request.code_head_sha
            || assessment.approved_spec_hash != request.approved_spec_hash
        {
            return Err("runtime QA applicability assessment is stale for this request".into());
        }
        let run = safe_uuid(&request.qa_run_id)?;
        let dir = self.root.join(run);
        fs::create_dir_all(&dir)
            .map_err(|error| format!("cannot create runtime QA record directory: {error}"))?;
        create_or_match_json(&dir.join("request.json"), request)?;
        create_json(&dir.join("applicability.json"), assessment)
    }

    pub fn load_applicability(
        &self,
        qa_run_id: &str,
    ) -> Result<super::QaApplicabilityAssessmentV1, String> {
        let run = safe_uuid(qa_run_id)?;
        let dir = self.root.join(run);
        let request: QaRunRequestV1 = read_json(&dir.join("request.json"))?;
        request.validate()?;
        if request.qa_run_id != run {
            return Err("runtime QA record path does not match its request identity".into());
        }
        let assessment: super::QaApplicabilityAssessmentV1 =
            read_json(&dir.join("applicability.json"))?;
        assessment.validate()?;
        if assessment.task_uid != request.parent_task_uid
            || assessment.code_sha != request.code_head_sha
            || assessment.approved_spec_hash != request.approved_spec_hash
        {
            return Err("persisted runtime QA applicability assessment is stale".into());
        }
        Ok(assessment)
    }

    pub fn save_disposition(&self, disposition: &QaFindingDispositionV1) -> Result<(), String> {
        disposition.validate()?;
        let run = safe_uuid(&disposition.qa_run_id)?;
        let finding = safe_uuid(&disposition.finding_id)?;
        let (request, _, evidence, result) = self.load_run(run)?;
        if request.parent_task_uid != disposition.parent_task_id
            || request.code_head_sha != disposition.code_sha
            || !result
                .findings
                .iter()
                .any(|item| item.finding_uid == finding)
            || evidence.code_sha != disposition.code_sha
        {
            return Err("runtime QA disposition does not match a persisted finding".into());
        }
        let path = self
            .root
            .join(run)
            .join("dispositions")
            .join(format!("{finding}.json"));
        create_or_match_json(&path, disposition)
    }

    /// Load the immutable Manager routing decisions for one validated run.
    pub fn load_dispositions(
        &self,
        qa_run_id: &str,
    ) -> Result<Vec<QaFindingDispositionV1>, String> {
        let (request, _, evidence, result) = self.load_run(qa_run_id)?;
        let directory = self.root.join(safe_uuid(qa_run_id)?).join("dispositions");
        if !directory.exists() {
            return Ok(Vec::new());
        }
        let mut paths = fs::read_dir(&directory)
            .map_err(|error| format!("cannot read runtime QA dispositions: {error}"))?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("cannot enumerate runtime QA dispositions: {error}"))?;
        paths.sort();
        paths
            .into_iter()
            .map(|path| {
                let disposition: QaFindingDispositionV1 = read_json(&path)?;
                disposition.validate()?;
                let finding = result
                    .findings
                    .iter()
                    .find(|finding| finding.finding_uid == disposition.finding_id)
                    .ok_or_else(|| {
                        "runtime QA disposition references an absent finding".to_owned()
                    })?;
                if disposition.qa_run_id != qa_run_id
                    || disposition.parent_task_id != request.parent_task_uid
                    || disposition.code_sha != evidence.code_sha
                    || disposition
                        .impacted_requirements
                        .iter()
                        .any(|requirement| !finding.requirement_uids.contains(requirement))
                {
                    return Err("runtime QA disposition is stale or mismatched".into());
                }
                Ok(disposition)
            })
            .collect()
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let bytes = fs::read(path).map_err(|error| {
        format!(
            "cannot read persisted runtime QA record {}: {error}",
            path.display()
        )
    })?;
    serde_json::from_slice(&bytes).map_err(|error| {
        format!(
            "persisted runtime QA record {} is malformed: {error}",
            path.display()
        )
    })
}

fn create_or_match_json<T: serde::Serialize + serde::de::DeserializeOwned + PartialEq>(
    path: &Path,
    value: &T,
) -> Result<(), String> {
    match read_json::<T>(path) {
        Ok(existing) if &existing == value => Ok(()),
        Ok(_) => Err("runtime QA request identity is already bound to different content".into()),
        Err(_error) if !path.exists() => create_json(path, value),
        Err(error) => Err(error),
    }
}

fn safe_uuid(value: &str) -> Result<&str, String> {
    if uuid::Uuid::parse_str(value).is_ok_and(|id| id.hyphenated().to_string() == value) {
        Ok(value)
    } else {
        Err("runtime QA record identifier is invalid".into())
    }
}

fn create_json(path: &Path, value: &(impl serde::Serialize + ?Sized)) -> Result<(), String> {
    let bytes =
        serde_json::to_vec_pretty(value).map_err(|_| "runtime QA record cannot be encoded")?;
    crate::artifacts::atomic_create_bytes(path, &bytes)
        .map_err(|error| format!("cannot create immutable runtime QA record: {error}"))
}
