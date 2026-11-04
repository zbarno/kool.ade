use crate::core::state::PlannerState;

pub(super) struct ApprovedSourcePin {
    pub(super) repository_id: String,
    pub(super) source_ref: Option<String>,
    pub(super) source_sha: String,
}

pub(super) fn approved_source_pin(
    state: &PlannerState,
    feature_uid: &str,
) -> anyhow::Result<Option<ApprovedSourcePin>> {
    let Some(run) = crate::core::self_improvement::SelfImprovementStore::new(&state.planning_store)
        .run_for_feature(feature_uid)?
    else {
        return Ok(None);
    };
    let workspace_uid = state.planning_store.project_id.hyphenated().to_string();
    let manifest = crate::core::project_repos::ProjectManifest::load(&state.planning_store)?;
    let repository = manifest
        .repositories
        .iter()
        .find(|repository| {
            crate::core::project_repos::stable_uid(&workspace_uid, "repository", &repository.id)
                == run.target_repository_uid
        })
        .ok_or_else(|| {
            anyhow::anyhow!("Approved self-improvement repository is missing from the manifest")
        })?;
    Ok(Some(ApprovedSourcePin {
        repository_id: repository.id.clone(),
        source_ref: run.source_ref,
        source_sha: run.source_sha,
    }))
}

pub(super) fn repository_source_sha(
    repository_id: &str,
    checkout: &std::path::Path,
    branch: &str,
    approved_pin: Option<&ApprovedSourcePin>,
) -> anyhow::Result<String> {
    if let Some(pin) = approved_pin.filter(|pin| pin.repository_id == repository_id) {
        anyhow::ensure!(
            pin.source_ref
                .as_deref()
                .is_none_or(|source_ref| source_ref == branch),
            "Approved self-improvement source branch changed before task generation"
        );
        return Ok(pin.source_sha.clone());
    }
    source_sha(checkout, branch)
}

fn source_sha(checkout: &std::path::Path, branch: &str) -> anyhow::Result<String> {
    let local_ref = format!("refs/heads/{branch}^{{commit}}");
    let local = crate::core::git_commands::validated_git_command(checkout)?
        .args(["rev-parse", "--verify", &local_ref])
        .output()?;
    if local.status.success() {
        return Ok(String::from_utf8_lossy(&local.stdout).trim().to_owned());
    }
    let remote = crate::core::gitops::origin_url(checkout)
        .ok_or_else(|| anyhow::anyhow!("Source branch {branch} has no local ref or live remote"))?;
    let output = crate::core::git_commands::validated_git_command(checkout)?
        .args([
            "ls-remote",
            "--heads",
            &remote,
            &format!("refs/heads/{branch}"),
        ])
        .output()?;
    anyhow::ensure!(
        output.status.success(),
        "Cannot verify source branch {branch} on its remote"
    );
    output
        .stdout
        .split(|byte| byte.is_ascii_whitespace())
        .next()
        .filter(|sha| matches!(sha.len(), 40 | 64) && sha.iter().all(u8::is_ascii_hexdigit))
        .map(|sha| String::from_utf8_lossy(sha).into_owned())
        .ok_or_else(|| anyhow::anyhow!("Source branch {branch} has no live remote commit"))
}

pub(super) fn validate_destination_ref(
    checkout: &std::path::Path,
    intent: &crate::core::git_intent::GitIntentV1,
) -> anyhow::Result<()> {
    let valid = crate::core::git_commands::validated_git_command(checkout)?
        .args(["check-ref-format", "--branch", &intent.destination_ref])
        .status()?;
    anyhow::ensure!(
        valid.success(),
        "Approved destination branch ref is invalid"
    );
    if let Some(remote) = crate::core::gitops::origin_url(checkout) {
        let reference = format!("refs/heads/{}", intent.destination_ref);
        let output = crate::core::git_commands::validated_git_command(checkout)?
            .args(["ls-remote", "--heads", &remote, &reference])
            .output()?;
        anyhow::ensure!(
            output.status.success() && !output.stdout.is_empty(),
            "Approved destination branch is not available on the repository remote"
        );
    } else {
        let reference = format!("refs/heads/{}^{{commit}}", intent.destination_ref);
        let output = crate::core::git_commands::validated_git_command(checkout)?
            .args(["rev-parse", "--verify", &reference])
            .output()?;
        anyhow::ensure!(
            output.status.success(),
            "Approved destination branch is not available in the local repository"
        );
    }
    Ok(())
}
