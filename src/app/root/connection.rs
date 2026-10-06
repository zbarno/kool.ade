use super::*;

impl KooladeApp {
    pub(super) fn submit_task_reply(&mut self, key: &str) {
        let review_changes = matches!(&self.screen, Screen::Connected(project)
            if project.implementation_states.get(key).is_some_and(|state|
                state.status == crate::core::implementation::ImplementationStatus::ChangesRequested));
        if review_changes {
            self.submit_review_changes(key);
            return;
        }
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
        if let Err(error) = crate::core::task_conversation::prompt(&project.state, key, &text, &[])
        {
            self.toasts.warning(error);
            return;
        }
        project.bind_task_conversation_identities();
        project.task_chats.ensure_loaded(&project.chat_slug);
        let user_message = ChatMessage::new(ChatRole::User, &text, Some(key.into()));
        let sent_id = user_message.id.clone();
        if let Err(error) = project
            .task_chats
            .append(&project.chat_slug, key, vec![user_message])
        {
            self.toasts.warning(error);
            return;
        }
        project.activity.pending.push(format!(
            "User replied in task {key}: {}",
            crate::core::context_build::clip(&text, 1600)
        ));
        // append merges the current on-disk history under a lock, so the prompt
        // also sees replies saved by another window since the last refresh.
        let recent_chat = project
            .task_chats
            .messages
            .get(key)
            .into_iter()
            .flatten()
            .filter(|m| m.id != sent_id)
            .map(|m| (format!("{:?}", m.role), m.text.clone()))
            .collect();
        let work = crate::core::planning_work::find(&project.state, key);
        let purpose = work
            .as_ref()
            .filter(|work| work.kind == crate::core::planning_work::WorkKind::Question)
            .map(|_| crate::core::workflow::TurnPurpose::Question)
            .unwrap_or(crate::core::workflow::TurnPurpose::Interview);
        let inputs = crate::core::turn::TurnInputs {
            state: project.state.clone(),
            user_message: text,
            recent_chat,
            purpose,
            comparison_feature: None,
        };
        project.task_chats.drafts.remove(key);
        let work_type = work
            .as_ref()
            .filter(|work| work.kind == crate::core::planning_work::WorkKind::DocumentationRefresh)
            .map(|_| crate::persistence::harness_settings::DOCUMENTATION);
        let task_routes = work
            .as_ref()
            .map(|work| work.routing_overrides.clone())
            .unwrap_or_default();
        let harness = configured_harness_for_task(&mut self.task_harness, work_type, &task_routes);
        let route_label = harness.label();
        project.task_turns.insert(
            key.into(),
            std::rc::Rc::new(TurnController::start_scoped(
                inputs,
                harness,
                Some(key.into()),
            )),
        );
        project.task_live.insert(
            key.into(),
            crate::harness::LiveProgress {
                selected_route: Some(route_label),
                ..Default::default()
            },
        );
    }

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
    /// Start a GitHub clone of `raw` (the connect card's URL field). Runs
    /// synchronously: a blank value is a silent no-op, an unparseable URL
    /// sets the banner and spawns NOTHING (no thread, no subprocess —
    /// quirk-bearing strings never reach git), and a valid target hands the
    /// CANONICAL rebuilt url to a background worker that clones into
    /// `$HOME/{repo}`.
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
                let repo = target.repo.clone();
                let job_repo = repo.clone();
                let join = if let Some(compute) = self.clone_computation_override.clone() {
                    std::thread::spawn(move || compute(canonical, job_repo))
                } else {
                    std::thread::spawn(move || welcome::perform_clone(&canonical, &job_repo))
                };
                self.clone_job = Some(CloneJob {
                    url_display,
                    repo,
                    join,
                });
            }
        }
    }

    /// The card's 'Clone' entry point: feed the FIELD's value (trimmed) to
    /// [`Self::begin_clone`].
    pub(super) fn begin_clone_from_field(&mut self) {
        let raw = self.conn_github.trim().to_string();
        self.begin_clone(&raw);
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

    pub(super) fn submit_connect(&mut self) {
        if self.conn_path.trim().is_empty() {
            return;
        }
        match welcome::attempt_connect(&self.conn_path) {
            Ok(mut project) => {
                self.attention.clear();
                self.setup_attention = None;
                self.setup_attention_notified = None;
                self.refresh_setup_attention(false);
                self.refresh_derived(&project);
                // F7 accrual (AD-4): engage per-workspace metering for this
                // session; the ledger file stays at the project root.
                crate::core::time_accrual::activate_project(
                    &project.state.repo_root,
                    &project.state.repositories,
                    &self.session_id,
                );
                project.remember_chat(vec![session::welcome_message(&project.state.title)]);
                self.conn_error = None;
                let title = project.state.title.clone();
                self.screen = Screen::Connected(Box::new(project));
                // Success: forget the pasted URL (per-launch state only).
                // A FAILED connect preserves it for typo correction.
                self.conn_github.clear();
                self.toasts.success(format!("Connected to {title}"));
            }
            Err(e) => {
                self.conn_error = Some(Self::conn_banner(&e));
            }
        }
    }
}
