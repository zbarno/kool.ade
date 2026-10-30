//! Planning-root manifest and private machine checkout map.
use crate::artifacts::planning_store::PlanningRoot;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

mod names;
mod save;
pub use names::{display_labels, normalize_display_name};
pub use save::{save_display_names, save_display_names_expected};

pub const PROJECT_FILE: &str = crate::artifacts::layout::store::PROJECT_MANIFEST;
pub const MAX_PROJECT_MANIFEST_BYTES: usize = 1024 * 1024;

/// Derive the portable identity used for repository and evidence references in
/// persisted opportunity proposals. The planning workspace scopes each ID.
pub fn stable_uid(workspace_uid: &str, kind: &str, value: &str) -> String {
    use sha2::Digest;

    let digest = sha2::Sha256::digest(format!("{workspace_uid}\0{kind}\0{value}").as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    uuid::Uuid::from_bytes(bytes).hyphenated().to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repository {
    pub id: String,
    pub role: String,
    #[serde(default)]
    pub remote: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectManifest {
    pub repositories: Vec<Repository>,
}
impl ProjectManifest {
    pub fn load<R: PlanningRoot + ?Sized>(planning_root: &R) -> anyhow::Result<Self> {
        let code_root = planning_root.code_repository_root();
        Self::load_with_optional_code_root(planning_root, code_root.as_deref())
    }

    pub fn load_with_code_root<R: PlanningRoot + ?Sized>(
        planning_root: &R,
        code_root: &Path,
    ) -> anyhow::Result<Self> {
        Self::load_with_optional_code_root(planning_root, Some(code_root))
    }

    fn load_with_optional_code_root<R: PlanningRoot + ?Sized>(
        planning_root: &R,
        code_root: Option<&Path>,
    ) -> anyhow::Result<Self> {
        let path = planning_root.planning_layout().project_manifest();
        let contents = match planning_root.read_planning_path(&path) {
            Ok(bytes) => {
                anyhow::ensure!(
                    bytes.len() <= MAX_PROJECT_MANIFEST_BYTES,
                    "project repository manifest exceeds the 1 MiB size limit"
                );
                String::from_utf8(bytes)?
            }
            Err(crate::artifacts::planning_store::StoreError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                anyhow::ensure!(
                    code_root.is_some(),
                    "A code repository root is required to initialize project repository identity"
                );
                let remote = code_root
                    .and_then(git_remote)
                    .filter(|remote| portable_remote(remote))
                    .unwrap_or_default();
                return Ok(Self {
                    repositories: vec![Repository {
                        id: "root".into(),
                        role: "Planning root".into(),
                        remote,
                        display_name: None,
                    }],
                });
            }
            Err(error) => {
                // Keep the offending path and OS cause visible: this used to
                // surface as a bare "No such file or directory" with no clue
                // which planning artifact was involved.
                return Err(anyhow::anyhow!(
                    "project repository manifest could not be read: {error:?}"
                ));
            }
        };
        let mut value: Self = serde_json::from_str(&contents)?;
        value.normalize_display_names()?;
        value.validate()?;
        Ok(value)
    }
    pub fn normalize_display_names(&mut self) -> anyhow::Result<()> {
        for repository in &mut self.repositories {
            repository.display_name = match repository.display_name.as_deref() {
                Some(name) => normalize_display_name(name)?,
                None => None,
            };
        }
        Ok(())
    }
    pub fn validate(&self) -> anyhow::Result<()> {
        let mut ids = HashSet::new();
        for repo in &self.repositories {
            if let Some(name) = &repo.display_name {
                anyhow::ensure!(
                    normalize_display_name(name)?.as_deref() == Some(name.as_str()),
                    "Repository {} display name must be trimmed",
                    repo.id
                );
            }
            anyhow::ensure!(valid_id(&repo.id), "Invalid repository ID {}", repo.id);
            anyhow::ensure!(ids.insert(&repo.id), "Duplicate repository ID {}", repo.id);
            anyhow::ensure!(
                !repo.role.trim().is_empty(),
                "Repository {} has no role",
                repo.id
            );
            anyhow::ensure!(
                repo.remote.is_empty() || portable_remote(&repo.remote),
                "Repository {} remote must be a portable repository identity",
                repo.id
            );
        }
        Ok(())
    }
    pub fn target(&self, planning_root: &Path, id: &str) -> anyhow::Result<PathBuf> {
        self.target_if_available(planning_root, id)?
            .ok_or_else(|| anyhow::anyhow!("No local checkout mapped for {id}"))
    }

    pub fn target_if_available(
        &self,
        planning_root: &Path,
        id: &str,
    ) -> anyhow::Result<Option<PathBuf>> {
        anyhow::ensure!(valid_id(id), "Invalid repository ID");
        let expected = self
            .repositories
            .iter()
            .find(|r| r.id == id)
            .ok_or_else(|| anyhow::anyhow!("Unknown repository ID {id}"))?;
        let root = planning_root.canonicalize()?;
        if crate::core::managed_workspace::WorkspaceManifest::is_managed_repository(&root)? {
            let workspace = crate::core::managed_workspace::WorkspaceManifest::read(&root)?;
            if let Some(checkout) =
                crate::core::managed_workspace::WorkspaceManager::from_state_root().checkout_path(
                    workspace.workspace_id,
                    id,
                    &expected.remote,
                )?
            {
                return Ok(Some(checkout));
            }
            return Ok(None);
        }
        if (id == "root" && expected.remote.is_empty())
            || git_remote(&root).as_deref() == Some(expected.remote.as_str())
        {
            return Ok(Some(root));
        }
        let mapping = private_checkout_map(&root)?;
        let Some(path) = mapping.get(id) else {
            return Ok(None);
        };
        let path = match PathBuf::from(path).canonicalize() {
            Ok(path) => path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        anyhow::ensure!(
            git_remote(&path).as_deref() == Some(expected.remote.as_str()),
            "Local checkout for {id} has a different origin"
        );
        Ok(Some(path))
    }
}

pub(crate) fn portable_remote(remote: &str) -> bool {
    if remote != remote.trim()
        || remote.is_empty()
        || remote.chars().any(char::is_control)
        || remote.starts_with('/')
        || remote.starts_with('-')
        || remote.contains('\\')
        || is_remote_helper(remote)
    {
        return false;
    }
    if let Some((scheme, address)) = remote.split_once("://") {
        return matches!(
            scheme.to_ascii_lowercase().as_str(),
            "http" | "https" | "ssh" | "git"
        ) && !address.is_empty();
    }
    let (host, path) = if remote.starts_with('[') {
        let Some((host, path)) = remote.rsplit_once("]:") else {
            return false;
        };
        (host, path)
    } else {
        let Some((host, path)) = remote.split_once(':') else {
            return false;
        };
        (host, path)
    };
    !host.is_empty() && !host.contains('/') && !host.starts_with('.') && !path.is_empty()
}

fn is_remote_helper(remote: &str) -> bool {
    remote.starts_with("ext::")
        || remote.split_once("::").is_some_and(|(transport, _)| {
            !transport.contains('/') && !transport.contains('@') && !transport.contains('[')
        })
}

pub fn map_local_checkout(planning_root: &Path, id: &str, checkout: &Path) -> anyhow::Result<()> {
    let root = planning_root.canonicalize()?;
    let expected =
        if crate::core::managed_workspace::WorkspaceManifest::is_managed_repository(&root)? {
            let workspace = crate::core::managed_workspace::WorkspaceManifest::read(&root)?;
            return crate::core::managed_workspace::WorkspaceManager::from_state_root()
                .map_checkout(workspace.workspace_id, id, &checkout.canonicalize()?);
        } else {
            let manifest = ProjectManifest::load(
                &crate::artifacts::planning_store::PlanningStore::legacy_embedded(
                    uuid::Uuid::nil(),
                    &root,
                ),
            )?;
            manifest
                .repositories
                .iter()
                .find(|r| r.id == id)
                .ok_or_else(|| anyhow::anyhow!("Unknown repository ID {id}"))?
                .clone()
        };
    let checkout = checkout.canonicalize()?;
    anyhow::ensure!(
        expected.remote.is_empty()
            || git_remote(&checkout).as_deref() == Some(expected.remote.as_str()),
        "Checkout origin does not match registered repository {id}"
    );
    let mut mapping = private_checkout_map(&root)?;
    mapping.insert(id.to_string(), checkout.to_string_lossy().into_owned());
    let slug = crate::persistence::project_slug(&root);
    let path = crate::persistence::project_dir(&slug).join("repositories.json");
    std::fs::create_dir_all(path.parent().unwrap())?;
    crate::artifacts::atomic_write(&path, &serde_json::to_string_pretty(&mapping)?)
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 40
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}
fn git_remote(root: &Path) -> Option<String> {
    let out = std::process::Command::new("git")
        // The manifest records repository identity. `remote get-url` applies
        // local insteadOf transport rewrites, which may point at a mirror.
        .args(["config", "--get", "remote.origin.url"])
        .current_dir(root)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8(out.stdout).ok())??
        .trim()
        .to_string()
        .into()
}
fn private_checkout_map(root: &Path) -> anyhow::Result<std::collections::BTreeMap<String, String>> {
    let slug = crate::persistence::project_slug(root);
    let path = crate::persistence::project_dir(&slug).join("repositories.json");
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(serde_json::from_str(&text)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Default::default()),
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests;
