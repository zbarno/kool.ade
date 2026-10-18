use std::path::Path;

use super::super::{
    QaEvidenceManifestV1, QaRunRequestV1, QaScreenshotArtifactV1, validate_report,
    validate_screenshot_artifact,
};
use super::{RuntimeQaStore, read_json, safe_uuid};

impl RuntimeQaStore {
    /// Load and verify one screenshot page without decoding unrelated images.
    pub fn load_screenshot_gallery(
        &self,
        qa_run_id: &str,
        offset: usize,
        limit: usize,
    ) -> Result<
        (
            super::super::QaRunRequestV1,
            QaEvidenceManifestV1,
            Vec<QaScreenshotArtifactV1>,
        ),
        String,
    > {
        if !(1..=24).contains(&limit) || offset > 100_000 {
            return Err("runtime QA screenshot page is outside its bounds".into());
        }
        let run = safe_uuid(qa_run_id)?;
        let directory = self.root.join(run);
        let request: QaRunRequestV1 = read_json(&directory.join("request.json"))?;
        let scenarios: Vec<super::super::QaScenarioV1> =
            read_json(&directory.join("scenarios.json"))?;
        let evidence: QaEvidenceManifestV1 = read_json(&directory.join("evidence.json"))?;
        let result: super::super::QaResultV1 = read_json(&directory.join("result.json"))?;
        if request.qa_run_id != qa_run_id {
            return Err("runtime QA record path does not match its request identity".into());
        }
        validate_report(&request, &scenarios, &evidence, &result)?;

        let Some(first) = evidence.screenshots.first() else {
            return Ok((request, evidence, Vec::new()));
        };
        let parent = Path::new(&first.storage_ref)
            .parent()
            .ok_or_else(|| "runtime QA screenshot path is invalid".to_owned())?;
        let planning = self.planning_store();
        let files = planning
            .list_files(parent)
            .map_err(|error| format!("cannot inspect local runtime QA screenshots: {error}"))?;
        let mut total_bytes = 0_u64;
        for metadata in &evidence.screenshots {
            let name = Path::new(&metadata.storage_ref)
                .file_name()
                .ok_or_else(|| "runtime QA screenshot path is invalid".to_owned())?
                .to_string_lossy();
            let file = files
                .iter()
                .find(|file| file.name == name)
                .ok_or_else(|| "runtime QA screenshot artifact is missing".to_owned())?;
            total_bytes = total_bytes.saturating_add(file.bytes);
            if file.bytes > request.bounded_limits.max_evidence_bytes
                || total_bytes > request.bounded_limits.max_evidence_bytes
            {
                return Err(
                    "persisted runtime QA screenshot set exceeds its evidence quota".into(),
                );
            }
        }

        let artifacts = evidence
            .screenshots
            .iter()
            .skip(offset)
            .take(limit)
            .map(|metadata| {
                let bytes = planning
                    .read(Path::new(&metadata.storage_ref))
                    .map_err(|error| format!("cannot read local runtime QA screenshot: {error}"))?;
                validate_screenshot_artifact(
                    metadata,
                    &bytes,
                    request.bounded_limits.max_evidence_bytes,
                )?;
                Ok(QaScreenshotArtifactV1 {
                    id: metadata.id.clone(),
                    bytes,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok((request, evidence, artifacts))
    }

    pub(super) fn write_screenshot(&self, path: &str, bytes: &[u8]) -> Result<(), String> {
        let planning = self.planning_store();
        match planning.atomic_create(path, bytes) {
            Ok(()) => Ok(()),
            Err(_)
                if self
                    .read_screenshot(path, bytes.len() as u64)
                    .is_ok_and(|existing| existing == bytes) =>
            {
                Ok(())
            }
            Err(error) => Err(format!(
                "cannot persist immutable local runtime QA screenshot: {error}"
            )),
        }
    }

    pub(super) fn read_screenshot(&self, path: &str, max_bytes: u64) -> Result<Vec<u8>, String> {
        let planning = self.planning_store();
        let relative = Path::new(path);
        let parent = relative
            .parent()
            .ok_or_else(|| "runtime QA screenshot path is invalid".to_owned())?;
        let file_name = relative
            .file_name()
            .ok_or_else(|| "runtime QA screenshot path is invalid".to_owned())?
            .to_string_lossy();
        let entry = planning
            .list_files(parent)
            .map_err(|error| format!("cannot inspect local runtime QA screenshots: {error}"))?
            .into_iter()
            .find(|entry| entry.name == file_name)
            .ok_or_else(|| "runtime QA screenshot artifact is missing".to_owned())?;
        if entry.bytes > max_bytes {
            return Err("runtime QA screenshot artifact exceeds its evidence quota".into());
        }
        planning
            .read(relative)
            .map_err(|error| format!("cannot read local runtime QA screenshot: {error}"))
    }

    pub(super) fn planning_store(&self) -> crate::artifacts::planning_store::PlanningStore {
        crate::artifacts::planning_store::PlanningStore::new(
            uuid::Uuid::nil(),
            self.root.clone(),
            crate::artifacts::planning_store::StoreMode::ManagedLocal,
        )
    }
}

pub(super) fn validate_screenshot_set(
    request: &QaRunRequestV1,
    evidence: &QaEvidenceManifestV1,
    screenshots: &[QaScreenshotArtifactV1],
) -> Result<(), String> {
    if screenshots.len() != evidence.screenshots.len()
        || screenshots
            .iter()
            .map(|screenshot| screenshot.id.as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != screenshots.len()
    {
        return Err("runtime QA screenshot artifacts are incomplete or duplicated".into());
    }
    let metadata_ids = evidence
        .screenshots
        .iter()
        .map(|screenshot| screenshot.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    if screenshots
        .iter()
        .any(|screenshot| !metadata_ids.contains(screenshot.id.as_str()))
    {
        return Err("runtime QA screenshot artifact has no manifest record".into());
    }
    let mut total_bytes = 0_u64;
    for metadata in &evidence.screenshots {
        let artifact = screenshots
            .iter()
            .find(|artifact| artifact.id == metadata.id)
            .ok_or_else(|| "runtime QA screenshot bytes are missing".to_owned())?;
        total_bytes = total_bytes.saturating_add(artifact.bytes.len() as u64);
        validate_screenshot_artifact(
            metadata,
            &artifact.bytes,
            request.bounded_limits.max_evidence_bytes,
        )?;
    }
    if total_bytes > request.bounded_limits.max_evidence_bytes {
        return Err("runtime QA screenshot set exceeds its aggregate evidence quota".into());
    }
    Ok(())
}
