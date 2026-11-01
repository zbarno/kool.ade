use super::*;

mod documentation_refresh;
mod task_reply;
mod task_retry;

impl KooladeApp {
    fn submit_review_changes(&mut self, key: &str) {
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        if project.task_turns.contains_key(key) {
            return;
        }
        let text = project
            .task_chats
            .drafts
            .get(key)
            .cloned()
            .unwrap_or_default();
        if text.trim().is_empty() {
            return;
        }
        project.bind_task_conversation_identities();
        project.task_chats.ensure_loaded(&project.chat_slug);
        if project
            .task_chats
            .append(
                &project.chat_slug,
                key,
                vec![ChatMessage::new(ChatRole::User, &text, Some(key.into()))],
            )
            .is_err()
        {
            self.toasts
                .warning("The requested changes could not be saved to this task's conversation.");
            return;
        }
        project.task_chats.drafts.remove(key);
        project.activity.pending.push(format!(
            "Requested changes for task {key}: {}",
            crate::core::context_build::clip(&text, 1600)
        ));
        self.start_implementation(key.to_owned(), true);
    }

    // ---------------------------------------------------------------- actions
    /// Start a GitHub clone of `raw` (the wizard's repository URL). Runs
    /// synchronously: a blank value is a silent no-op, an unparseable URL
    /// sets the banner and spawns NOTHING (no thread, no subprocess —
    /// quirk-bearing strings never reach git), and a valid target hands the
    /// CANONICAL rebuilt url to a background worker that clones into
    /// `$HOME/kool-ade-workspaces/{repo}`, then pairs planning before connect.
    pub(super) fn begin_clone(&mut self, raw: &str, project_title: &str) {
        let url = raw.trim();
        if url.is_empty() {
            return;
        }
        // Defense in depth: the busy card already steals every signal (the
        // primary guard); refuse here too so a stray duplicate can never
        // leak the in-flight JoinHandle nor double-book the scratch name.
        if self.clone_job.is_some() {
            return;
        }
        match welcome::parse_github_url(url) {
            Err(detail) => {
                self.conn_error = Some(format!("Can't clone that URL\n{detail}"));
            }
            Ok(target) => {
                let url_display = format!("github.com/{}/{}", target.owner, target.repo);
                let canonical = target.url.clone();
                let repo = target.repo;
                let title = if project_title.trim().is_empty() {
                    repo.clone()
                } else {
                    project_title.trim().to_owned()
                };
                let join = if let Some(compute) = self.clone_computation_override.clone() {
                    std::thread::spawn(move || compute(canonical, title))
                } else {
                    std::thread::spawn(move || {
                        welcome::perform_clone_and_pair(&canonical, &title)
                            .map_err(|error| crate::error::AppError::Other(format!("{error:#}")))
                    })
                };
                self.clone_job = Some(CloneJob {
                    url_display,
                    repo,
                    join,
                });
            }
        }
    }

    /// Single banner formatter for every connect/clone failure (headline
    /// over detail) so the submit path and the clone-completion path
    /// cannot drift apart.
    pub(super) fn conn_banner(e: &crate::error::AppError) -> String {
        format!("{}\n{}", e.headline(), e.detail())
    }

    /// Surfaces buffered time-ledger write failures on the connected
    /// project's activity rail (metering is best-effort: surfacing the
    /// problem beats silent data loss).
    pub(super) fn surface_time_ledger_errors(&mut self) {
        let notes = crate::core::time_accrual::drain_errors();
        if notes.is_empty() {
            return;
        }
        if let Screen::Connected(project) = &mut self.screen {
            for note in notes {
                project.activity.pending.push(note);
            }
        }
    }

    pub(super) fn submit_open_local_workspace(&mut self) {
        if self.clone_job.is_some() {
            return;
        }
        let Some(id) = self.conn_workspace_id else {
            self.conn_error = Some("No local workspace is available to open.".into());
            return;
        };
        self.finish_connect(welcome::attempt_open_workspace(id));
    }

    /// Wizard "new project" track: creates the code repository at the chosen
    /// location and keeps planning history in Kool.ad/e's managed workspace area.
    /// Maps an operator-facing project title onto a safe repository folder
    /// name: words separated by hyphens, lower-cased, ASCII letters/digits
    /// only, forced to start with a letter so the pair validator accepts it.
    pub(super) fn project_title_to_folder_name(title: &str) -> String {
        // Tokenize on runs of ASCII alphanumerics; every other character
        // acts as a separator, so "2.0" becomes "2-0" rather than "20".
        let mut segments: Vec<String> = Vec::new();
        let mut current = String::new();
        for character in title.chars() {
            if character.is_ascii_alphanumeric() {
                current.push(character.to_ascii_lowercase());
            } else if !current.is_empty() {
                segments.push(std::mem::take(&mut current));
            }
        }
        if !current.is_empty() {
            segments.push(current);
        }
        let name = segments.join("-");
        match name.as_str() {
            "" => "project".to_owned(),
            _ if !name.starts_with(|c: char| c.is_ascii_alphabetic()) => format!("project-{name}"),
            _ => name,
        }
    }

    /// Choose a parent folder only for a new blank project. Existing code and
    /// planning checkouts are never selected from disk in setup.
    pub(super) fn pick_wizard_folder(&mut self, which: WizField) {
        let seed = self.wiz_seed_for(which);
        let current = std::path::PathBuf::from(&seed);
        let start = if current.is_dir() {
            Some(current.clone())
        } else {
            current
                .parent()
                .filter(|parent| parent.is_dir())
                .map(std::path::Path::to_path_buf)
        }
        .or_else(|| std::env::current_dir().ok());
        let mut picker = rfd::FileDialog::new().set_title("Choose the folder for the new project");
        if let Some(start) = start {
            picker = picker.set_directory(start);
        }
        self.apply_wizard_folder_pick(which, picker.pick_folder());
    }

    /// Field-write half of the parent-folder picker for deterministic UI tests.
    pub(in crate::app::root) fn apply_wizard_folder_pick(
        &mut self,
        which: WizField,
        picked: Option<std::path::PathBuf>,
    ) {
        let Some(path) = picked else {
            return;
        };
        let path = path.canonicalize().unwrap_or(path);
        let path = path.to_string_lossy().into_owned();
        self.wiz_apply_field(which, path);
    }

    pub(super) fn finish_connect(
        &mut self,
        result: Result<session::Project, crate::error::AppError>,
    ) {
        match result {
            Ok(mut project) => {
                self.attention.clear();
                self.setup_attention.clear();
                self.setup_attention_notified = None;
                self.refresh_setup_attention(false);
                self.refresh_derived(&project);
                // F7 accrual (AD-4): engage per-workspace metering for this
                // session; the ledger file stays at the project root.
                crate::core::time_accrual::activate_project(
                    &project.state.repo_root,
                    project.state.planning_store.project_id,
                    &project.state.repositories,
                    &self.session_id,
                );
                project.remember_chat(vec![session::welcome_message(&project.state.title)]);
                self.conn_error = None;
                // Wizard completions land here too: the welcome journey
                // is OVER wherever the connect succeeded from.
                self.wiz = None;
                let title = project.state.title.clone();
                if let Some(workspace) = &project.managed_workspace {
                    let id = workspace.id;
                    let manager =
                        crate::core::managed_workspace::WorkspaceManager::from_state_root();
                    match manager.queue_successful_open(id) {
                        Ok(()) => self.launch_history_jobs.push(std::thread::spawn(move || {
                            manager.flush_pending_opens().map_err(|error| error.to_string())
                        })),
                        Err(_) => self.toasts.warning("Could not save recent workspace history. Check access to application storage."),
                    }
                }
                self.screen = Screen::Connected(Box::new(project));
                self.toasts.success(format!("Connected to {title}"));
            }
            Err(e) => {
                // A refused connection (typically a missing planning
                // repository) still warrants showing local environment
                // readiness so setup can proceed.
                self.refresh_setup_attention(false);
                self.conn_error = Some(Self::conn_banner(&e));
            }
        }
    }
}

#[cfg(test)]
#[path = "connection/tests.rs"]
mod tests;

#[cfg(test)]
mod title_slug_tests {
    use super::KooladeApp;

    #[test]
    fn folder_name_slugging_edges() {
        assert_eq!(
            KooladeApp::project_title_to_folder_name("Acme Portal"),
            "acme-portal"
        );
        assert_eq!(
            KooladeApp::project_title_to_folder_name("Uber Cool 2.0 Draft"),
            "uber-cool-2-0-draft"
        );
        // Pure punctuation degrades to the stable fallback, never a bare
        // dash or an empty segment.
        assert_eq!(
            KooladeApp::project_title_to_folder_name("!!! ???"),
            "project"
        );
        assert_eq!(
            KooladeApp::project_title_to_folder_name("42 Ways"),
            "project-42-ways"
        );
    }
}
