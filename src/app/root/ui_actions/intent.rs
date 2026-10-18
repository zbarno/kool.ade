use super::super::{Dialog, KooladeApp, Screen};
use crate::ui::{HeaderAction, Intent, Surface};

impl KooladeApp {
    pub(super) fn apply_user_intent(&mut self, intent: Intent) {
        if let Some((id, candidate_hash)) = &intent.request_feature_changes {
            self.request_feature_changes(id, candidate_hash);
            return;
        }
        if let Some(id) = &intent.approve_feature {
            if let Screen::Connected(project) = &mut self.screen {
                project.remember_chat(vec![crate::domain::ChatMessage::new(
                    crate::domain::ChatRole::System,
                    format!("Open {id}'s review card on the Kanban to read and approve its current specification."),
                    None,
                )]);
            }
            return;
        }
        if intent.implement_tasks {
            if self.implementation_offer() {
                super::super::requested_action::start_next(self);
            }
            return;
        }
        if intent.generate_tasks {
            if let Screen::Connected(project) = &mut self.screen {
                project.remember_chat(vec![crate::domain::ChatMessage::new(
                    crate::domain::ChatRole::System,
                    "Start task generation from the approved feature's Generate tasks card on the Kanban.",
                    None,
                )]);
            }
            return;
        }
        if intent.cancel {
            self.pending_feature_generation = None;
            if let Screen::Connected(project) = &mut self.screen {
                project.activity.manager = None;
                if let Some(controller) = &project.active_turn {
                    controller.request_cancel();
                    self.toasts.warning("Cancellation requested…");
                }
            }
            return;
        }
        if intent.send {
            let taken = match &mut self.screen {
                Screen::Connected(project) => std::mem::take(&mut project.draft),
                Screen::Welcome => return,
            };
            let text = taken.trim();
            if !text.is_empty() {
                self.start_turn(text);
            }
        }
    }

    pub(super) fn apply_header_action(&mut self, action: HeaderAction) {
        match action {
            HeaderAction::Refresh => {
                if let Screen::Connected(project) = &mut self.screen {
                    project.last_pr_refresh = None;
                    project.refresh_git();
                    project.refresh_implementations();
                    project.task_documents = crate::artifacts::task_docs::load_board(
                        &project.state.planning_store,
                        &project.state.workflow,
                    );
                    self.toasts.info("Git state refreshed");
                }
            }
            HeaderAction::Import => {
                self.dialog = Some(Dialog::Import(super::super::DlgImport::new()))
            }
            HeaderAction::Stakeholders => {
                if let Screen::Connected(project) = &self.screen {
                    self.dialog = Some(Dialog::Settings(super::super::DlgSettings::from_project(
                        project,
                    )));
                }
            }
            HeaderAction::McpServers => {
                if let Screen::Connected(project) = &self.screen {
                    self.dialog = Some(Dialog::Mcp(super::super::DlgMcp::from_project(project)));
                }
            }
            HeaderAction::CopySpec => self.copy_spec_to_clipboard(),
            HeaderAction::OpenWorkspace => self.open_workspace(),
            HeaderAction::OpenRegisteredRepository { id } => self.open_registered_repository(&id),
            HeaderAction::AddCodeRepository => {
                if self.can_add_code_repository() {
                    self.dialog = Some(Dialog::AddCodeRepository(
                        super::super::app_dialogs::DlgCodeRepository::default(),
                    ));
                } else {
                    self.toasts
                        .warning("Code repositories can be added to managed planning workspaces.");
                }
            }
            HeaderAction::MigrateEmbeddedPlanning => self.migrate_embedded_planning(),
            HeaderAction::RollbackPlanningMigration => self.rollback_planning_migration(),
            HeaderAction::Disconnect => self.disconnect(),
        }
    }

    fn migrate_embedded_planning(&mut self) {
        let Some(path) = (match &self.screen {
            Screen::Connected(project) => Some(project.state.repo_root.clone()),
            Screen::Welcome => None,
        }) else {
            return;
        };
        let manager = crate::core::managed_workspace::WorkspaceManager::from_state_root();
        match manager.migrate_legacy_checkout(&path) {
            Ok(workspace) => {
                let summary = migration_discovery_summary(&path, &workspace);
                let selected = path.to_string_lossy().into_owned();
                self.finish_connect(crate::app::welcome::attempt_connect(&selected));
                match summary {
                    Ok(summary) => self.toasts.success(summary),
                    Err(error) => self.toasts.warning(format!(
                        "Migration completed, but its discovery report could not be read: {error:#}"
                    )),
                }
            }
            Err(error) => self
                .toasts
                .warning(format!("Planning migration stopped safely: {error:#}")),
        }
    }

    fn rollback_planning_migration(&mut self) {
        let Some(path) = (match &self.screen {
            Screen::Connected(project) => Some(project.state.repo_root.clone()),
            Screen::Welcome => None,
        }) else {
            return;
        };
        let manager = crate::core::managed_workspace::WorkspaceManager::from_state_root();
        match manager.rollback_legacy_migration(&path) {
            Ok(outcome) => {
                let selected = path.to_string_lossy().into_owned();
                self.finish_connect(crate::app::welcome::attempt_connect(&selected));
                if let Some(warning) = outcome.warning {
                    self.toasts.warning(warning);
                }
            }
            Err(error) => self
                .toasts
                .warning(format!("Planning rollback did not run: {error:#}")),
        }
    }
}

fn migration_discovery_summary(
    source_root: &std::path::Path,
    workspace: &crate::core::managed_workspace::ManagedWorkspace,
) -> anyhow::Result<String> {
    use crate::core::managed_workspace::MigrationReport;

    let report: MigrationReport = serde_json::from_slice(&std::fs::read(
        workspace.planning_repo.join("migration-report.json"),
    )?)?;
    Ok(format_migration_discovery_summary(source_root, &report))
}

fn format_migration_discovery_summary(
    source_root: &std::path::Path,
    report: &crate::core::managed_workspace::MigrationReport,
) -> String {
    let sources = report
        .source_repositories
        .iter()
        .map(|source| {
            let commit: String = source.source_commit.chars().take(8).collect();
            let branch_count = report
                .source_branches
                .iter()
                .filter(|branch| branch.repository_id == source.repository_id)
                .count();
            format!(
                "{} · {} @ {} · {} artifacts · {} branch snapshots{}",
                source.repository_id,
                source.source_ref,
                commit,
                source.artifact_count,
                branch_count,
                if source.available {
                    ""
                } else {
                    " · unavailable"
                },
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "Migration complete. Source root: {}\n{}",
        source_root.display(),
        sources
    )
}

#[cfg(test)]
mod tests {
    use super::format_migration_discovery_summary;

    #[test]
    fn migration_summary_shows_source_root_refs_commits_and_counts() {
        let report: crate::core::managed_workspace::MigrationReport =
            serde_json::from_value(serde_json::json!({
                "schemaVersion": 1,
                "workspaceId": "11111111-1111-4111-8111-111111111111",
                "sourceCommit": "0123456789abcdef",
                "sourceRef": "main",
                "sourceRepositories": [{
                    "repositoryId": "root",
                    "sourceCommit": "0123456789abcdef",
                    "sourceRef": "main",
                    "artifactCount": 4,
                    "available": true
                }],
                "sourceArtifacts": [],
                "sourceBranches": [{
                    "repositoryId": "root",
                    "sourceRef": "feature/demo",
                    "sourceCommit": "fedcba9876543210",
                    "sourcePath": "planning/items/example.md",
                    "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "disposition": "shared"
                }],
                "generatedArtifacts": []
            }))
            .unwrap();
        let summary =
            format_migration_discovery_summary(std::path::Path::new("/test/source"), &report);
        assert!(summary.contains("Source root: /test/source"));
        assert!(summary.contains("root · main @ 01234567 · 4 artifacts · 1 branch snapshots"));
    }
}
