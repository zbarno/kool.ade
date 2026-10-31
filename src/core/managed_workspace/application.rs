//! Consent-driven creation of separate application and planning repositories.
use super::{CodeRepositoryRegistration, ManagedWorkspace, WorkspaceManager, git, locking};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};
mod intent;
pub(super) mod preflight;
pub(super) mod publish;
pub(crate) use publish::directory as publish_directory;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplicationCreation {
    pub transaction_id: uuid::Uuid,
    pub workspace_id: uuid::Uuid,
    pub title: String,
    pub intent: String,
    pub code_directory: PathBuf,
}

impl WorkspaceManager {
    /// Run only after the operator approves these exact locations and inputs.
    /// The private receipt preserves inputs, not permission to replay on restart.
    /// No app commit, package install, remote operation or rollback deletion runs.
    pub fn create_application(
        &self,
        request: &ApplicationCreation,
    ) -> anyhow::Result<ManagedWorkspace> {
        anyhow::ensure!(
            !request.title.trim().is_empty() && request.title.len() <= 160,
            "enter a short project name"
        );
        anyhow::ensure!(
            !request.intent.trim().is_empty() && request.intent.len() <= 16_384,
            "enter a short description of what you want to make"
        );
        let code = &request.code_directory;
        let parent = code
            .parent()
            .ok_or_else(|| anyhow::anyhow!("choose a project parent folder"))?;
        anyhow::ensure!(
            parent.canonicalize()? == parent && code.file_name().is_some(),
            "choose an absolute project location without redirected parent folders"
        );
        let state_root = preflight::canonical_location(&self.home)?;
        anyhow::ensure!(
            !code.starts_with(&state_root) && !state_root.starts_with(code),
            "application code and planning history must be separate, non-nested locations"
        );
        preflight::outside_repository(parent)?;
        let transactions = self.home.join("application-creations");
        fs::create_dir_all(&transactions)?;
        let _lock = locking::lock(&transactions.join(format!("{}.lock", request.transaction_id)))?;
        let receipt = transactions.join(format!("{}.json", request.transaction_id));
        let resume = receipt.is_file();
        if resume {
            let saved: ApplicationCreation = serde_json::from_slice(&fs::read(&receipt)?)?;
            anyhow::ensure!(
                saved == *request,
                "creation inputs changed; review a new transaction"
            );
        } else {
            anyhow::ensure!(
                fs::symlink_metadata(code).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
                "project destination already exists or cannot be inspected"
            );
            crate::artifacts::atomic_create_bytes(&receipt, &serde_json::to_vec_pretty(request)?)?;
        }
        let marker = request.transaction_id.to_string();
        if code.exists() {
            anyhow::ensure!(
                resume && !fs::symlink_metadata(code)?.file_type().is_symlink(),
                "project destination is occupied"
            );
            verify_marker(code, &marker)?;
        } else {
            let staging = parent.join(format!(".koolade-create-{}", request.transaction_id));
            match fs::create_dir(&staging) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    let metadata = fs::symlink_metadata(&staging)?;
                    anyhow::ensure!(
                        metadata.is_dir() && !metadata.file_type().is_symlink(),
                        "creation staging location is not a regular directory"
                    );
                }
                Err(error) => return Err(error.into()),
            }
            let ownership = staging.join(".koolade-creation");
            if ownership.is_file() {
                anyhow::ensure!(
                    fs::read_to_string(&ownership)? == marker,
                    "staging belongs to another creation"
                );
            } else if staging.join(".git").exists() {
                // Only our completed marker can authorize recovery after the
                // temporary ownership file was removed before publication.
                verify_marker(&staging, &marker)?;
            } else {
                anyhow::ensure!(
                    fs::read_dir(&staging)?.next().is_none(),
                    "staging contains unrelated files"
                );
                crate::artifacts::atomic_create_bytes(&ownership, marker.as_bytes())?;
            }
            for entry in fs::read_dir(&staging)? {
                let name = entry?.file_name();
                anyhow::ensure!(
                    name == ".git" || name == ".koolade-creation",
                    "staging contains unexpected files"
                );
            }
            if !staging.join(".git").exists() {
                git::init_planning_repo(&staging)?;
            }
            anyhow::ensure!(
                fs::symlink_metadata(staging.join(".git"))?.is_dir(),
                "staged Git metadata is not a directory"
            );
            git::run(
                Some(&staging),
                &["config", "--local", "koolade.creationId", &marker],
            )?;
            super::creation::sync_tree(&staging)?;
            if ownership.exists() {
                fs::remove_file(&ownership)?;
            }
            super::manager::sync_directory(&staging)?;
            publish::directory(&staging, code)?;
            super::manager::sync_directory(parent)?;
        }
        verify_marker(code, &marker)?;
        self.create_identified(request.workspace_id, &request.title)?;
        let workspace = self.register_code_repository(
            request.workspace_id,
            CodeRepositoryRegistration {
                id: format!("app-{}", request.transaction_id.simple()),
                role: "application".into(),
                remote: None,
                checkout: code.clone(),
            },
        )?;
        intent::preserve(&workspace, request)?;
        Ok(workspace)
    }
}

fn verify_marker(path: &std::path::Path, marker: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        git::repository_root(path)?.canonicalize()? == path.canonicalize()?,
        "application repository boundary changed"
    );
    let observed = git::run(
        Some(path),
        &["config", "--local", "--get", "koolade.creationId"],
    )?;
    anyhow::ensure!(
        String::from_utf8(observed.stdout)?.trim() == marker,
        "destination does not belong to this creation transaction"
    );
    Ok(())
}
