use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::super::QaRunRequestV1;

const MAX_ARTIFACTS: usize = 32;
const MAX_ARTIFACT_BYTES: usize = 12 * 1024;
const MAX_TOTAL_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct QaScenarioArtifactEvidenceV1 {
    pub path: String,
    pub content: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct QaScenarioSourceEvidenceV1 {
    pub artifacts: Vec<QaScenarioArtifactEvidenceV1>,
    pub sha256: String,
}

impl QaScenarioSourceEvidenceV1 {
    pub(crate) fn from_prepared(
        task_clone: &std::path::Path,
        artifact_paths: &[String],
        artifact_hashes: &std::collections::BTreeMap<String, String>,
    ) -> Result<Self, String> {
        let root = task_clone
            .canonicalize()
            .map_err(|error| format!("verified Runtime QA task clone is unavailable: {error}"))?;
        if artifact_paths.is_empty() || artifact_paths.len() > MAX_ARTIFACTS {
            return Err(
                "the approved build has no bounded text artifacts for scenario synthesis".into(),
            );
        }
        let mut total = 0usize;
        let mut artifacts = Vec::with_capacity(artifact_paths.len());
        for relative in artifact_paths {
            let path = std::path::Path::new(relative);
            if path.is_absolute()
                || path
                    .components()
                    .any(|component| !matches!(component, std::path::Component::Normal(_)))
            {
                return Err("approved build artifact path is not repository-relative".into());
            }
            let mut cursor = root.clone();
            for component in path.components() {
                cursor.push(component.as_os_str());
                let metadata = std::fs::symlink_metadata(&cursor)
                    .map_err(|error| format!("approved build artifact is unavailable: {error}"))?;
                if metadata.file_type().is_symlink() {
                    return Err(
                        "scenario synthesis does not follow symlinked build artifacts".into(),
                    );
                }
            }
            let resolved = cursor.canonicalize().map_err(|error| error.to_string())?;
            if !resolved.starts_with(&root) || !resolved.is_file() {
                return Err(
                    "approved build artifact escaped the task clone or is not a file".into(),
                );
            }
            let bytes = std::fs::read(&resolved).map_err(|error| error.to_string())?;
            total = total.saturating_add(bytes.len());
            if bytes.is_empty() || bytes.len() > MAX_ARTIFACT_BYTES || total > MAX_TOTAL_BYTES {
                return Err(
                    "approved build text artifacts exceed scenario synthesis limits".into(),
                );
            }
            let content = String::from_utf8(bytes.clone()).map_err(|_| {
                "binary build artifacts cannot ground QA scenario synthesis".to_owned()
            })?;
            if content.contains('\0') {
                return Err("build artifact contains invalid text".into());
            }
            let sha256 = digest(&bytes);
            if artifact_hashes.get(relative) != Some(&sha256) {
                return Err("approved build artifact changed since its passing receipt".into());
            }
            artifacts.push(QaScenarioArtifactEvidenceV1 {
                path: relative.clone(),
                content,
                sha256,
            });
        }
        Self::seal(artifacts)
    }

    fn seal(mut artifacts: Vec<QaScenarioArtifactEvidenceV1>) -> Result<Self, String> {
        artifacts.sort_by(|left, right| left.path.cmp(&right.path));
        let mut evidence = Self {
            artifacts,
            sha256: String::new(),
        };
        evidence.sha256 =
            digest(&serde_json::to_vec(&evidence).map_err(|error| error.to_string())?);
        Ok(evidence)
    }

    pub(crate) fn validate_for(&self, request: &QaRunRequestV1) -> Result<(), String> {
        request.validate()?;
        self.validate_integrity()
    }

    fn validate_integrity(&self) -> Result<(), String> {
        if self.artifacts.is_empty() || self.artifacts.len() > MAX_ARTIFACTS {
            return Err("scenario synthesis evidence has no bounded build artifacts".into());
        }
        let mut total = 0usize;
        let mut previous: Option<&str> = None;
        for artifact in &self.artifacts {
            let bytes = artifact.content.as_bytes();
            total += bytes.len();
            if artifact.path.is_empty()
                || artifact.sha256 != digest(bytes)
                || bytes.is_empty()
                || bytes.len() > MAX_ARTIFACT_BYTES
                || total > MAX_TOTAL_BYTES
                || previous.is_some_and(|path| path >= artifact.path.as_str())
            {
                return Err("scenario synthesis artifact evidence is invalid or oversized".into());
            }
            previous = Some(&artifact.path);
        }
        let mut unsigned = self.clone();
        let expected = unsigned.sha256.clone();
        unsigned.sha256.clear();
        if expected != digest(&serde_json::to_vec(&unsigned).map_err(|error| error.to_string())?) {
            return Err("scenario synthesis evidence digest does not match its content".into());
        }
        Ok(())
    }
}

#[cfg(test)]
pub(super) fn test_fixture() -> QaScenarioSourceEvidenceV1 {
    test_fixture_with_content("<main>approved page</main>")
}

#[cfg(test)]
pub(super) fn test_fixture_with_content(content: &str) -> QaScenarioSourceEvidenceV1 {
    QaScenarioSourceEvidenceV1::seal(vec![QaScenarioArtifactEvidenceV1 {
        path: "dist/index.html".into(),
        content: content.to_owned(),
        sha256: digest(content.as_bytes()),
    }])
    .unwrap()
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence() -> QaScenarioSourceEvidenceV1 {
        QaScenarioSourceEvidenceV1::seal(vec![QaScenarioArtifactEvidenceV1 {
            path: "dist/index.html".into(),
            content: "<main>approved page</main>".into(),
            sha256: digest(b"<main>approved page</main>"),
        }])
        .unwrap()
    }

    #[test]
    fn source_evidence_digest_binds_text_and_rejects_mutation() {
        let evidence = evidence();
        evidence.validate_integrity().unwrap();
        let mut changed = evidence.clone();
        changed.artifacts[0]
            .content
            .push_str("<script>bad</script>");
        assert!(changed.validate_integrity().is_err());
    }

    #[test]
    fn evidence_reader_accepts_only_receipt_verified_regular_text_artifacts() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("dist")).unwrap();
        let bytes = b"<main>approved page</main>";
        std::fs::write(temp.path().join("dist/index.html"), bytes).unwrap();
        let path = "dist/index.html".to_owned();
        let hashes = [(path.clone(), digest(bytes))].into();
        let evidence = QaScenarioSourceEvidenceV1::from_prepared(
            temp.path(),
            std::slice::from_ref(&path),
            &hashes,
        )
        .unwrap();
        assert_eq!(
            evidence.artifacts[0].content,
            String::from_utf8_lossy(bytes)
        );
        let stale = [(path.clone(), digest(b"old bytes"))].into();
        assert!(
            QaScenarioSourceEvidenceV1::from_prepared(
                temp.path(),
                std::slice::from_ref(&path),
                &stale,
            )
            .is_err()
        );
    }
}
