macro_rules! project_surface_methods {
    () => {
        fn planning_repository(&self) -> Option<crate::ui::PlanningRepositoryViewModel> {
            self.planning_repository_view()
        }

        fn save_planning_remote(&mut self, remote: &str) -> Result<(), String> {
            self.update_planning_remote(remote)
        }

        fn people_settings_draft(&self) -> Option<crate::app::dialogs::DlgSettings> {
            match &self.screen {
                Screen::Connected(project) => {
                    Some(crate::app::dialogs::DlgSettings::from_project(project))
                }
                Screen::Welcome => None,
            }
        }

        fn save_people_settings(
            &mut self,
            draft: &mut crate::app::dialogs::DlgSettings,
        ) -> Result<String, String> {
            let Screen::Connected(project) = &mut self.screen else {
                return Err("Connect a workspace before editing its people settings.".into());
            };
            let sha = draft.apply(project).map_err(|error| error.detail())?;
            (self.cached_user, self.synth) = Self::derive_caches(project);
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

        fn registered_repositories(&self) -> Vec<crate::ui::RepositoryChoice> {
            repository_switcher::choices(&self.screen)
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
                Screen::Connected(project) if project.git.default_branch.is_empty() => {
                    if project.git.branch.is_empty() {
                        "main"
                    } else {
                        &project.git.branch
                    }
                }
                Screen::Connected(project) => &project.git.default_branch,
                Screen::Welcome => "",
            }
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
