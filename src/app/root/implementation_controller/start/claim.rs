use crate::{
    artifacts::task_docs::TaskDocument,
    core::task_claim::{ClaimError, ClaimRequest},
};

pub(super) fn prepare(
    project_uid: uuid::Uuid,
    documents: &[TaskDocument],
    ticket: &str,
) -> Result<ClaimRequest, ClaimError> {
    let task_uid = documents
        .iter()
        .find(|document| document.path == ticket)
        .and_then(|document| document.identity.as_ref())
        .map(|identity| identity.uid.clone())
        .unwrap_or_else(|| path_identity(ticket));
    Ok(ClaimRequest::new(project_uid, task_uid))
}

/// Keep documents without a stable embedded UID in the same local fence slot
/// by deriving a path key without reading or importing any remote claim data.
fn path_identity(ticket: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(ticket.as_bytes());
    let hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("path-{hex}")
}
