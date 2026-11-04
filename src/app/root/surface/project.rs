macro_rules! project_surface_methods {
    () => {
        fn local_user_settings_draft(&self) -> Option<crate::app::dialogs::DlgSettings> {
            match &self.screen {
                Screen::Connected(project) => {
                    Some(crate::app::dialogs::DlgSettings::from_project(project))
                }
                Screen::Welcome => None,
            }
        }

        fn save_local_user_settings(
            &mut self,
            draft: &mut crate::app::dialogs::DlgSettings,
        ) -> Result<String, String> {
            let Screen::Connected(project) = &mut self.screen else {
                return Err("Connect a workspace before editing local identity settings.".into());
            };
            let sha = draft.apply(project).map_err(|error| error.detail())?;
            self.cached_user = Self::derive_caches(project);
            self.toasts.success(format!("Saved · checkpoint {sha}"));
            Ok(sha)
        }

        fn project_settings_draft(&self) -> Option<crate::app::dialogs::DlgProjectSettings> {
            match &self.screen {
                Screen::Connected(project) => Some(
                    crate::app::dialogs::DlgProjectSettings::from_project(project),
                ),
                Screen::Welcome => None,
            }
        }

        fn save_project_settings(
            &mut self,
            draft: &mut crate::app::dialogs::DlgProjectSettings,
        ) -> Result<Option<String>, String> {
            let Screen::Connected(project) = &mut self.screen else {
                return Err("Connect a workspace before editing Project & Git settings.".into());
            };
            let sha = draft.apply(project).map_err(|error| error.detail())?;
            if let Some(sha) = &sha {
                self.toasts.success(format!("Saved · checkpoint {sha}"));
            } else if draft.agent_guidance_saved {
                self.toasts.success("Saved project guidance in private local settings");
            } else {
                self.toasts.info("Project settings already in sync");
            }
            Ok(sha)
        }

        fn session_title(&self) -> &str {
            match &self.screen {
                Screen::Connected(p) => p.state.title.as_str(),
                Screen::Welcome => "",
            }
        }

        fn is_git_repo(&self) -> bool {
            matches!(&self.screen, Screen::Connected(project) if project.has_selected_code_repository())
        }

        fn git_branch(&self) -> &str {
            match &self.screen {
                Screen::Connected(p) if p.has_selected_code_repository() => {
                    p.git.branch.as_str()
                }
                Screen::Welcome => "",
                Screen::Connected(_) => "",
            }
        }

        fn git_checkout_label(&self) -> String {
            let Screen::Connected(project) = &self.screen else {
                return "Primary code repository".into();
            };
            let Ok(manifest) = crate::core::project_repos::ProjectManifest::load(
                &project.state.planning_store,
            ) else {
                return "Primary code repository".into();
            };
            let labels = crate::core::project_repos::display_labels(&manifest.repositories);
            manifest
                .repositories
                .iter()
                .position(|repository| repository.id == "root")
                .and_then(|index| labels.get(index).cloned())
                .unwrap_or_else(|| "Primary code repository".into())
        }

        fn repository_branches(&self) -> Vec<String> {
            match &self.screen {
                Screen::Connected(project) if !project.has_selected_code_repository() => {
                    Vec::new()
                }
                Screen::Connected(project) if project.git.branches.is_empty() => {
                    (!project.git.branch.is_empty())
                        .then(|| project.git.branch.clone())
                        .into_iter()
                        .collect()
                }
                Screen::Connected(project) => project.git.branches.clone(),
                Screen::Welcome => Vec::new(),
            }
        }

        fn repository_destination_branches(&self) -> Vec<String> {
            match &self.screen {
                Screen::Connected(project) if !project.has_selected_code_repository() => {
                    Vec::new()
                }
                Screen::Connected(project) if !project.git.has_origin => self.repository_branches(),
                Screen::Connected(project) => project.git.remote_branches.clone(),
                Screen::Welcome => Vec::new(),
            }
        }

        fn default_repository_branch(&self) -> &str {
            match &self.screen {
                Screen::Connected(project) if !project.has_selected_code_repository() => "",
                Screen::Connected(project) => &project.git.default_branch,
                Screen::Welcome => "",
            }
        }

        fn repository_git_options(&self) -> Vec<crate::ui::RepositoryGitOptions> {
            let Screen::Connected(project) = &self.screen else {
                return Vec::new();
            };
            let Ok(manifest) = crate::core::project_repos::ProjectManifest::load(
                &project.state.planning_store,
            ) else {
                return Vec::new();
            };
            let labels = crate::core::project_repos::display_labels(&manifest.repositories);
            manifest
                .repositories
                .iter()
                .zip(labels)
                .map(|(repository, label)| {
                    let checkout = manifest.target_if_available(
                        &project.state.planning_store,
                        &project.state.repo_root,
                        &repository.id,
                    );
                    match checkout {
                        Ok(Some(checkout)) => {
                            let git = crate::core::gitops::snapshot(&checkout);
                            let destinations = if git.has_origin {
                                git.remote_branches.clone()
                            } else {
                                git.branches.clone()
                            };
                            crate::ui::RepositoryGitOptions {
                                id: repository.id.clone(),
                                label,
                                source_branches: git.branches,
                                destination_branches: destinations,
                                default_branch: git.default_branch,
                                unavailable_reason: None,
                            }
                        }
                        Ok(None) => crate::ui::RepositoryGitOptions {
                            id: repository.id.clone(),
                            label,
                            unavailable_reason: Some("No local checkout is mapped".into()),
                            ..Default::default()
                        },
                        Err(error) => crate::ui::RepositoryGitOptions {
                            id: repository.id.clone(),
                            label,
                            unavailable_reason: Some(error.to_string()),
                            ..Default::default()
                        },
                    }
                })
                .collect()
        }

        fn git_head(&self) -> &str {
            match &self.screen {
                Screen::Connected(p) if p.has_selected_code_repository() => {
                    p.git.head_short.as_str()
                }
                Screen::Welcome => "",
                Screen::Connected(_) => "",
            }
        }

        fn git_dirty(&self) -> bool {
            match &self.screen {
                Screen::Connected(p) => p.has_selected_code_repository() && p.git.dirty > 0,
                Screen::Welcome => false,
            }
        }
    };
}

pub(super) use project_surface_methods;
