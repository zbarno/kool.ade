use super::{QaEvidenceManifestV1, QaResultV1, QaRunRequestV1};
use sha2::Digest;

/// Bind independent review to the immutable request, the candidate's
/// approved-scenario hash, and observed evidence.
pub fn qa_review_subject_sha256(
    request: &QaRunRequestV1,
    candidate: &QaResultV1,
    evidence: &QaEvidenceManifestV1,
) -> Result<String, String> {
    let bytes = serde_json::to_vec(&(request, candidate, evidence))
        .map_err(|error| format!("runtime QA review subject cannot be encoded: {error}"))?;
    Ok(format!("{:x}", sha2::Sha256::digest(bytes)))
}
