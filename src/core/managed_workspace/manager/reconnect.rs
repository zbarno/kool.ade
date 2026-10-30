use super::*;

impl WorkspaceManager {
    /// Explicitly reconnect a missing checkout after verifying the same UUID
    /// and ancestry of the last successfully opened commit. Never replace a
    /// still-present checkout or accept an unrelated/diverged planning history.
    pub fn reconnect(&self, id: uuid::Uuid, chosen: &Path) -> anyhow::Result<ManagedWorkspace> {
        let _lock = super::super::locking::lock(&self.project_dir(id).join(".workspace.lock"))?;
        let previous = WorkspaceAssociation::read(&self.association_path(id), id)?;
        anyhow::ensure!(
            fs::symlink_metadata(&previous.planning_repo_path)
                .is_err_and(|error| error.kind() == io::ErrorKind::NotFound),
            "the original planning location still exists or cannot be inspected; it was not replaced"
        );
        let candidate = chosen.canonicalize()?;
        anyhow::ensure!(
            git::repository_root(&candidate)? == candidate,
            "choose the planning repository root"
        );
        let manifest = WorkspaceManifest::read(&candidate)?;
        anyhow::ensure!(manifest.workspace_id == id, "this is a different workspace");
        let _repository_lock = crate::core::gitops::lock_repository(&candidate)?;
        let identity = self.relocation_identity(id)?.ok_or_else(|| anyhow::anyhow!(
            "no original-folder identity is available; restore the original location before opening"))?;
        anyhow::ensure!(
            RelocationIdentity::capture(&candidate)? == identity,
            "choose the original moved folder on the same filesystem; copies and backups require separate recovery"
        );
        let token = self.reconnect_token(id)?.ok_or_else(|| anyhow::anyhow!(
            "this workspace has no relocation marker; restore its original folder before opening it"))?;
        let git_dir = git::run(Some(&candidate), &["rev-parse", "--absolute-git-dir"])?;
        let marker = std::path::PathBuf::from(String::from_utf8(git_dir.stdout)?.trim())
            .join("koolade-relocation");
        anyhow::ensure!(
            fs::read_to_string(marker)?.trim() == token,
            "choose the original moved checkout; cloned replacements cannot prove continuity"
        );
        let expected = self.last_opened_head(id)?.ok_or_else(|| anyhow::anyhow!(
            "no verified prior planning revision is available; automatic reconnect cannot verify continuity"))?;
        let ancestry = git::output(
            Some(&candidate),
            &["merge-base", "--is-ancestor", &expected, "HEAD"],
        )?;
        anyhow::ensure!(
            ancestry.status.success(),
            "planning history diverged or cannot be verified"
        );
        let mut updated = previous.clone();
        updated.planning_repo_path = candidate;
        let workspace = Self::validate_association(id, updated.clone())?;
        anyhow::ensure!(
            RelocationIdentity::capture(&workspace.planning_repo)? == identity,
            "the selected folder changed during validation"
        );
        updated.write(&self.association_path(id))?;
        Ok(workspace)
    }
}
