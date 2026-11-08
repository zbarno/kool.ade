use crate::{
    artifacts::planning_store::PlanningStore, core::project_repos::ProjectManifest,
    domain::chatlog::VerifiedRepositoryEvidence,
};

pub(crate) fn validate_reviewed_repository_evidence(
    code_root: &std::path::Path,
    planning_store: &PlanningStore,
    repositories: &ProjectManifest,
    evidence_items: &[VerifiedRepositoryEvidence],
) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    for evidence in evidence_items {
        if evidence.evidence_id.len() > 160
            || evidence.source_ref.is_empty()
            || evidence.source_ref.len() > 255
            || !(40..=64).contains(&evidence.source_sha.len())
            || !evidence
                .source_sha
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || evidence.content_sha256.len() != 64
            || !evidence
                .content_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || evidence.content.len() > 2048
            || evidence.excerpt_sha256.len() != 64
            || format!("{:x}", Sha256::digest(evidence.content.as_bytes()))
                != evidence.excerpt_sha256
        {
            return Err("Reviewed repository evidence is malformed or has an invalid hash.".into());
        }
        let source_root = repositories
            .target_if_available(planning_store, code_root, &evidence.repository_id)
            .map_err(
                |_| "Reviewed evidence names a repository without a validated local checkout.",
            )?
            .ok_or("Reviewed evidence names a repository without a validated local checkout.")?;
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(source_root)
            .args(["rev-parse", "--verify", "--end-of-options"])
            .arg(format!("{}^{{commit}}", evidence.source_ref))
            .output()
            .map_err(|_| "Reviewed repository evidence could not be checked for freshness.")?;
        let resolved = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if !output.status.success() || resolved != evidence.source_sha {
            return Err("The reviewed repository revision changed. Run the code review again before using its findings.".into());
        }
    }
    Ok(())
}
