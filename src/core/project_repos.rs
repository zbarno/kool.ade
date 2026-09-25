//! Planning-root manifest and private machine checkout map.
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

pub const PROJECT_FILE: &str = crate::artifacts::layout::legacy::PROJECT_MANIFEST;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repository {
    pub id: String,
    pub role: String,
    pub remote: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectManifest {
    pub repositories: Vec<Repository>,
}
impl ProjectManifest {
    pub fn load(planning_root: &Path) -> anyhow::Result<Self> {
        let path = crate::artifacts::layout::ArtifactLayout::new(planning_root)
            .legacy_project_manifest();
        let contents = match std::fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let remote = git_remote(planning_root).unwrap_or_default();
                return Ok(Self {
                    repositories: vec![Repository {
                        id: "root".into(),
                        role: "Planning root".into(),
                        remote,
                    }],
                });
            }
            Err(e) => return Err(e.into()),
        };
        let value: Self = serde_json::from_str(&contents)?;
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.repositories.is_empty(),
            "Project repository manifest is empty"
        );
        let mut ids = HashSet::new();
        for repo in &self.repositories {
            anyhow::ensure!(valid_id(&repo.id), "Invalid repository ID {}", repo.id);
            anyhow::ensure!(ids.insert(&repo.id), "Duplicate repository ID {}", repo.id);
            anyhow::ensure!(
                !repo.role.trim().is_empty(),
                "Repository {} has no role",
                repo.id
            );
            anyhow::ensure!(
                !repo.remote.trim().is_empty(),
                "Repository {} has no remote identity",
                repo.id
            );
            anyhow::ensure!(
                !repo.remote.contains('\n')
                    && !repo.remote.starts_with('/')
                    && !repo.remote.starts_with("file://")
                    && !repo.remote.contains('\\'),
                "Repository {} remote must be a portable repository identity",
                repo.id
            );
        }
        Ok(())
    }
    pub fn target(&self, planning_root: &Path, id: &str) -> anyhow::Result<PathBuf> {
        anyhow::ensure!(valid_id(id), "Invalid repository ID");
        let expected = self
            .repositories
            .iter()
            .find(|r| r.id == id)
            .ok_or_else(|| anyhow::anyhow!("Unknown repository ID {id}"))?;
        let root = planning_root.canonicalize()?;
        if id == "root" && expected.remote.is_empty() {
            return Ok(root);
        }
        if git_remote(&root).as_deref() == Some(expected.remote.as_str()) {
            return Ok(root);
        }
        let mapping = private_checkout_map(&root)?;
        let path = mapping
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("No local checkout mapped for {id}"))?;
        let path = PathBuf::from(path).canonicalize()?;
        anyhow::ensure!(
            git_remote(&path).as_deref() == Some(expected.remote.as_str()),
            "Local checkout for {id} has a different origin"
        );
        Ok(path)
    }
}

pub fn map_local_checkout(planning_root: &Path, id: &str, checkout: &Path) -> anyhow::Result<()> {
    let root = planning_root.canonicalize()?;
    let manifest = ProjectManifest::load(&root)?;
    let expected = manifest
        .repositories
        .iter()
        .find(|r| r.id == id)
        .ok_or_else(|| anyhow::anyhow!("Unknown repository ID {id}"))?;
    let checkout = checkout.canonicalize()?;
    anyhow::ensure!(
        git_remote(&checkout).as_deref() == Some(expected.remote.as_str()),
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
        .args(["remote", "get-url", "origin"])
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
mod tests {
    use super::*;
    #[test]
    fn manifest_rejects_ambiguous_or_machine_specific_entries() {
        let bad = ProjectManifest {
            repositories: vec![
                Repository {
                    id: "api".into(),
                    role: "Backend".into(),
                    remote: "git@example/api".into(),
                },
                Repository {
                    id: "api".into(),
                    role: "Duplicate".into(),
                    remote: "git@example/other".into(),
                },
            ],
        };
        assert!(bad.validate().is_err());
        let path = Repository {
            id: "../mobile".into(),
            role: "Mobile".into(),
            remote: "/home/user/mobile".into(),
        };
        assert!(!valid_id(&path.id));
    }

    #[test]
    fn portable_manifest_resolves_private_checkout_without_committing_paths() {
        let root = std::env::temp_dir().join(format!(
            "packet-repos-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let planning = root.join("planning-root");
        let api = root.join("api-checkout");
        std::fs::create_dir_all(planning.join(".planner")).unwrap();
        std::fs::create_dir_all(&api).unwrap();
        for repo in [&planning, &api] {
            assert!(
                std::process::Command::new("git")
                    .args(["init", "-q"])
                    .current_dir(repo)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        assert!(
            std::process::Command::new("git")
                .args(["remote", "add", "origin", "git@example.test:team/api.git"])
                .current_dir(&api)
                .status()
                .unwrap()
                .success()
        );
        let manifest = ProjectManifest {
            repositories: vec![
                Repository {
                    id: "planning".into(),
                    role: "Planning root".into(),
                    remote: "git@example.test:team/planning.git".into(),
                },
                Repository {
                    id: "api".into(),
                    role: "Backend API".into(),
                    remote: "git@example.test:team/api.git".into(),
                },
            ],
        };
        std::fs::write(
            planning.join(PROJECT_FILE),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
        map_local_checkout(&planning, "api", &api).unwrap();
        assert_eq!(
            ProjectManifest::load(&planning)
                .unwrap()
                .target(&planning, "api")
                .unwrap(),
            api.canonicalize().unwrap()
        );
        assert!(
            !std::fs::read_to_string(planning.join(PROJECT_FILE))
                .unwrap()
                .contains(api.to_str().unwrap())
        );
        let private = crate::persistence::project_dir(&crate::persistence::project_slug(
            &planning.canonicalize().unwrap(),
        ));
        let _ = std::fs::remove_dir_all(private);
        let _ = std::fs::remove_dir_all(root);
    }
}
