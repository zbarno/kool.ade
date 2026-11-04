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
    /// Start a GitHub code-repository clone of `raw`. Runs
    /// synchronously: a blank value is a silent no-op, an unparseable URL
    /// sets the banner and spawns NOTHING (no thread, no subprocess —
    /// quirk-bearing strings never reach git), and a valid target hands the
    /// CANONICAL rebuilt url to a background worker that clones into
    /// `$HOME/kool-ade-workspaces/{repo}` before connect.
    pub(super) fn begin_clone(&mut self, raw: &str) {
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
                let worker_repo = repo.clone();
                let join = if let Some(compute) = self.clone_computation_override.clone() {
                    std::thread::spawn(move || compute(canonical, worker_repo))
                } else {
                    std::thread::spawn(move || welcome::perform_clone(&canonical, &worker_repo))
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
                self.screen = Screen::Connected(Box::new(project));
                self.toasts.success(format!("Connected to {title}"));
            }
            Err(e) => {
                // A refused connection still warrants showing local
                // environment readiness so setup can proceed.
                self.refresh_setup_attention(false);
                self.conn_error = Some(Self::conn_banner(&e));
            }
        }
    }
}

#[cfg(test)]
#[path = "connection/tests.rs"]
mod tests;
