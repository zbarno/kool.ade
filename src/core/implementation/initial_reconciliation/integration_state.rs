use super::*;

const FILE: &str = "base-reconciliation-integration.json";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IntegratedCandidate {
    schema_version: u8,
    task_repository: PathBuf,
    branch: String,
    base_commit: String,
    verified_head: String,
}

pub(in crate::core::implementation) fn mark_integrated_candidate(
    dir: &Path,
    candidate: &Implementation,
) -> anyhow::Result<()> {
    if dir.join(PLAN_FILE).exists() {
        let plan = read_plan(&dir.join(PLAN_FILE))?;
        anyhow::ensure!(
            plan.verified_commit.is_some(),
            "Cannot retire an unverified initial reconciliation plan"
        );
    }
    let verified_head = candidate
        .verified_head
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Integrated candidate is not verified"))?;
    let marker = IntegratedCandidate {
        schema_version: 1,
        task_repository: candidate.task_repository.canonicalize()?,
        branch: candidate.branch.clone(),
        base_commit: candidate.base_commit.clone(),
        verified_head: verified_head.to_owned(),
    };
    crate::artifacts::atomic_write_bytes(&dir.join(FILE), &serde_json::to_vec_pretty(&marker)?)
}

pub(in crate::core::implementation) fn integrated_candidate_matches(
    dir: &Path,
    state: &Implementation,
) -> anyhow::Result<bool> {
    let path = dir.join(FILE);
    if !path.exists() {
        return Ok(false);
    }
    anyhow::ensure!(
        fs::metadata(&path)?.len() <= 1024 * 1024,
        "Integrated reconciliation marker is too large; preserved for review"
    );
    let marker: IntegratedCandidate = serde_json::from_slice(&fs::read(path)?)?;
    anyhow::ensure!(
        marker.schema_version == 1,
        "Unsupported integrated reconciliation marker version"
    );
    Ok(state.task_repository_kind == TaskRepositoryKind::Clone
        && state.task_repository.canonicalize()? == marker.task_repository
        && state.branch == marker.branch
        && state.base_commit == marker.base_commit
        && state.verified_head.as_deref() == Some(marker.verified_head.as_str()))
}
