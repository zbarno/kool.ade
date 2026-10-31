//! Read-only observations for explaining interrupted project setup.
use super::{ApplicationCreation, WorkspaceManager};
use std::{io::ErrorKind, path::Path};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecoveryRequest {
    New(ApplicationCreation),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtifactPresence {
    Found,
    NotFound,
    Unavailable,
}

pub struct RecoveryItem {
    pub label: &'static str,
    pub presence: ArtifactPresence,
}

fn presence(path: &Path) -> ArtifactPresence {
    match std::fs::symlink_metadata(path) {
        Ok(_) => ArtifactPresence::Found,
        Err(error) if error.kind() == ErrorKind::NotFound => ArtifactPresence::NotFound,
        Err(_) => ArtifactPresence::Unavailable,
    }
}

impl WorkspaceManager {
    /// Observation only: existence does not establish ownership, valid Git
    /// history or permission to resume. The approved operation revalidates all.
    pub fn inspect_setup_recovery(&self, request: &RecoveryRequest) -> Vec<RecoveryItem> {
        use ArtifactPresence::{Found, NotFound, Unavailable};
        let RecoveryRequest::New(request) = request;
        let folder = &request.code_directory;
        let id = request.workspace_id;
        let repository_id = format!("app-{}", request.transaction_id.simple());
        let idea = request.transaction_id.to_string();
        let mut items = vec![
            RecoveryItem {
                label: "App folder",
                presence: presence(folder),
            },
            RecoveryItem {
                label: "App history entry",
                presence: presence(&folder.join(".git")),
            },
            RecoveryItem {
                label: "Local workspace connection",
                presence: presence(&self.association_path(id)),
            },
        ];
        if let Some(parent) = folder.parent() {
            items.push(RecoveryItem {
                label: "Temporary creation folder",
                presence: presence(
                    &parent.join(format!(".koolade-create-{}", request.transaction_id)),
                ),
            });
        }
        items.push(RecoveryItem {
            label: "Planning folder",
            presence: presence(&self.planning_repo_path(id)),
        });
        let workspace = Self::open_unlocked(self, id).ok();
        let registration =
            workspace.as_ref().map_or(Unavailable, |workspace| {
                if workspace.association.code_checkouts.iter().any(|checkout| {
                    checkout.repository_id == repository_id && checkout.path == *folder
                }) {
                    Found
                } else {
                    NotFound
                }
            });
        items.push(RecoveryItem {
            label: "Saved app connection",
            presence: registration,
        });
        {
            let work = workspace
                .as_ref()
                .and_then(|workspace| crate::core::planning_work::load(&workspace.store).ok());
            items.push(RecoveryItem {
                label: "Saved initial idea",
                presence: work.map_or(Unavailable, |work| {
                    if work.iter().any(|work| {
                        work.uid == idea
                            && work.kind == crate::core::planning_work::WorkKind::NewProject
                    }) {
                        Found
                    } else {
                        NotFound
                    }
                }),
            });
        }
        items
    }
}
