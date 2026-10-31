use super::*;

mod details;
mod plan_review;
mod runtime_qa;
mod setup_repair;

impl Surface for KooladeApp {
    runtime_qa::runtime_qa_surface_methods!();
    plan_review::plan_review_surface_methods!();

    fn setup_image_preparation(&mut self, ui: &mut egui::Ui, prerequisite_id: &str) {
        let (completed, request) = setup_repair::paint(self, ui, prerequisite_id);
        self.publish_setup_task_store_update();
        if completed {
            self.refresh_setup_attention(true);
        }
        if let Some(task_id) = request {
            self.dispatch_ui_command(crate::ui::ApplicationCommand::ApproveSetupTaskFix {
                task_id,
            });
        }
    }

    fn supports_provider_install(&self, task_id: &str) -> bool {
        task_id == "setup:cli:claude:install"
            && crate::app::bootstrap_remediation::ClaudeInstallController::supports_current_host()
    }

    fn setup_provider_install(&mut self, ui: &mut egui::Ui, task_id: &str) {
        if task_id != "setup:cli:claude:install" {
            return;
        }
        ui.add_space(crate::ui::theme::spacing::M);
        ui.label(crate::ui::theme::section_heading("Install Claude Code"));
        ui.label(
            "Review the exact Anthropic release, signature, checksum, and current-user file changes before anything is downloaded. Sign-in and model access remain separate provider steps.",
        );
        let request = self
            .claude_install
            .preview()
            .and_then(|preview| preview.show_confirmation(ui));
        let install_status = self.claude_install.status().clone();
        match install_status {
            crate::app::bootstrap_remediation::ClaudeInstallStatus::Running => {
                let phase = match self.claude_install.progress() {
                    Some(crate::app::bootstrap_remediation::ClaudeInstallProgress::Download) => {
                        "Downloading the pinned release"
                    }
                    Some(
                        crate::app::bootstrap_remediation::ClaudeInstallProgress::TrustVerification,
                    ) => "Verifying Anthropic's signature and checksum",
                    Some(
                        crate::app::bootstrap_remediation::ClaudeInstallProgress::CurrentUserInstall,
                    ) => "Installing for the current user",
                    Some(
                        crate::app::bootstrap_remediation::ClaudeInstallProgress::FreshProviderProbe,
                    ) => "Checking the installed Claude Code CLI",
                    None => "Preparing the approved install",
                };
                ui.label(phase);
                if ui.button("Cancel install").clicked() {
                    if let Err(error) = self.claude_install.cancel_review() {
                        self.toasts
                            .warning(format!("Could not cancel the provider install: {error}"));
                    }
                    self.publish_setup_task_store_update();
                }
            }
            crate::app::bootstrap_remediation::ClaudeInstallStatus::Finished(Ok(())) => {
                ui.label("The install completed and the CLI passed its fresh provider probe.");
            }
            crate::app::bootstrap_remediation::ClaudeInstallStatus::Finished(Err(_)) => {
                ui.label("The install did not complete. Review the setup task and try again.");
            }
            crate::app::bootstrap_remediation::ClaudeInstallStatus::Idle
            | crate::app::bootstrap_remediation::ClaudeInstallStatus::NeedsApproval => {}
        }
        if self.claude_install.preview().is_some() && ui.button("Cancel review").clicked() {
            if let Err(error) = self.claude_install.cancel_review() {
                self.toasts.warning(format!(
                    "Could not cancel the provider install review: {error}"
                ));
            }
            self.publish_setup_task_store_update();
            return;
        }
        if let Some(request) = request {
            match self.claude_install.approve(request) {
                Ok(_) => {
                    self.publish_setup_task_store_update();
                    self.toasts
                        .info("The approved Claude Code install is running for this user.");
                }
                Err(error) => self
                    .toasts
                    .warning(format!("The Claude Code install could not start: {error}")),
            }
        } else if self.claude_install.preview().is_none()
            && self.claude_install.status()
                == &crate::app::bootstrap_remediation::ClaudeInstallStatus::Idle
        {
            ui.label("Choose Fix for me to review the pinned install. Nothing downloads before you approve it.");
        }
    }

    fn bootstrap_conversation(
        &mut self,
        task_id: &str,
    ) -> crate::ui::planning_board::BootstrapConversationView {
        self.bootstrap_conversation.view(task_id)
    }

    fn bootstrap_conversation_draft(&mut self, task_id: &str) -> Option<&mut String> {
        self.bootstrap_conversation.draft_mut(task_id)
    }

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
            Screen::Connected(p) if p.has_selected_code_repository() => p.git.branch.as_str(),
            Screen::Welcome => "",
            Screen::Connected(_) => "",
        }
    }

    fn repository_branches(&self) -> Vec<String> {
        match &self.screen {
            Screen::Connected(project) if !project.has_selected_code_repository() => Vec::new(),
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
            Screen::Connected(project) if !project.has_selected_code_repository() => Vec::new(),
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
            Screen::Connected(p) if p.has_selected_code_repository() => p.git.head_short.as_str(),
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

    fn chat_messages(&self) -> &[ChatMessage] {
        match &self.screen {
            Screen::Connected(p) => p.chat.as_slice(),
            Screen::Welcome => &[],
        }
    }

    fn chat_draft(&mut self) -> &mut String {
        match &mut self.screen {
            Screen::Connected(p) => &mut p.draft,
            Screen::Welcome => &mut self.welcome_draft,
        }
    }

    fn task_messages(&self, key: &str) -> &[ChatMessage] {
        match &self.screen {
            Screen::Connected(p) => p
                .task_chats
                .messages
                .get(key)
                .map(Vec::as_slice)
                .unwrap_or(&[]),
            _ => &[],
        }
    }
    fn task_chat_context(&self, key: &str) -> Option<String> {
        let Screen::Connected(p) = &self.screen else {
            return None;
        };
        let (mut context, _) =
            crate::core::task_conversation::presentation(&p.state, &p.task_documents, key)?;
        if let Some(implementation) = p.implementation_states.get(key) {
            context.push_str(&format!(
                "\n\nImplementation: {}\n{}",
                implementation.status,
                crate::core::context_build::clip(&implementation.detail, 1600)
            ));
        }
        Some(context)
    }
    fn task_draft(&mut self, key: &str) -> Option<&mut String> {
        match &mut self.screen {
            Screen::Connected(p) => Some(p.task_chats.drafts.entry(key.into()).or_default()),
            _ => None,
        }
    }
    fn task_chat_active(&self, key: &str) -> bool {
        matches!(&self.screen, Screen::Connected(p) if p.task_turns.contains_key(key))
    }
    fn task_reply_progress(&self, key: &str) -> Option<&crate::harness::LiveProgress> {
        match &self.screen {
            Screen::Connected(p) => p.task_live.get(key),
            _ => None,
        }
    }
    fn task_chat_error(&self) -> Option<&str> {
        match &self.screen {
            Screen::Connected(p) => p.task_chats.error.as_deref(),
            _ => None,
        }
    }
    fn activity_samples(&self, key: Option<&str>) -> Vec<(i64, u64)> {
        self.activity_samples_for_surface(key)
    }
    fn activity_active(&self, key: &str) -> bool {
        self.implementation_active(key)
            || self.task_chat_active(key)
            || matches!(&self.screen,
            Screen::Connected(p) if p.investigation.as_ref().is_some_and(|run| run.item_id == key))
    }
    fn task_reply_busy(&self) -> bool {
        matches!(&self.screen, Screen::Connected(p) if p.active_turn.is_some())
    }

    fn is_busy(&self) -> bool {
        matches!(&self.screen, Screen::Connected(p) if p.active_turn.is_some() || !p.active_implementations.is_empty())
    }

    fn conversation_busy(&self) -> bool {
        matches!(&self.screen, Screen::Connected(p) if p.active_turn.is_some())
    }
    fn task_progress(&self, ticket: &str) -> Option<&crate::harness::LiveProgress> {
        match &self.screen {
            Screen::Connected(p) => p.activity.tasks.get(ticket),
            _ => None,
        }
    }
    fn max_parallel_tasks(&self) -> usize {
        match &self.screen {
            Screen::Connected(p) => p.queue.max_parallel.clamp(1, 8),
            _ => 3,
        }
    }
    fn active_task_count(&self) -> usize {
        match &self.screen {
            Screen::Connected(p) => p.active_implementations.len(),
            _ => 0,
        }
    }
    fn live_progress(&self) -> Option<&crate::harness::LiveProgress> {
        match &self.screen {
            Screen::Connected(p)
                if p.task_chats.active.is_none()
                    && (p.active_turn.is_some() || p.activity.manager.is_some()) =>
            {
                Some(&p.live_progress)
            }
            _ => None,
        }
    }

    fn active_planning_work(&self) -> Option<&str> {
        match &self.screen {
            Screen::Connected(project) if project.active_turn.is_some() => {
                project.active_planning_work.as_deref()
            }
            _ => None,
        }
    }

    fn task_offer(&self) -> Option<&crate::core::workflow::InterviewBrief> {
        match &self.screen {
            Screen::Connected(p)
                if p.active_turn.is_none()
                    && p.active_implementations.is_empty()
                    && !has_current_task_batch(p)
                    && p.state.workflow.ready(p.state.planning_contract()) =>
            {
                p.state.workflow.brief.as_ref()
            }
            _ => None,
        }
    }

    fn implementation_offer(&self) -> bool {
        matches!(&self.screen, Screen::Connected(p)
            if p.active_turn.is_none()
                && p.active_implementations.is_empty()
                && has_current_task_batch(p)
                && matches!(crate::core::implementation_queue::next_ready_ticket_with_lifecycle_subtasks(
                    &p.task_documents,
                    &p.implementation_states,
                    &Default::default(),
                    &Default::default(),
                    &crate::app::root::implementation_controller::completed_subtask_uids(p),
                ), Ok(Some(_))))
    }

    fn implementation_state(
        &self,
        ticket: &str,
    ) -> Option<&crate::core::implementation::Implementation> {
        match &self.screen {
            Screen::Connected(p) => p.implementation_states.get(ticket),
            _ => None,
        }
    }
    fn implementation_failure(&self, ticket: &str) -> Option<&str> {
        match &self.screen {
            Screen::Connected(p) => p
                .queue
                .blocked
                .get(ticket)
                .map(|failure| failure.message.as_str()),
            _ => None,
        }
    }
    fn implementation_recovery(
        &self,
        ticket: &str,
    ) -> Option<crate::core::implementation::RecoveryDisposition> {
        match &self.screen {
            Screen::Connected(project) => project
                .queue
                .blocked
                .get(ticket)
                .map(|failure| failure.recovery),
            _ => None,
        }
    }

    fn task_detail_view(&mut self, ticket: &str) -> Option<crate::ui::task_detail::ViewModel> {
        self.task_detail_for_surface(ticket)
    }
    fn dispatch(&mut self, command: crate::ui::ApplicationCommand) {
        self.dispatch_ui_command(command);
    }

    fn implementation_active(&self, ticket: &str) -> bool {
        matches!(&self.screen, Screen::Connected(p) if p.active_implementations.contains_key(ticket))
    }
    fn implementation_waiting_for_capacity(&self, ticket: &str) -> bool {
        matches!(&self.screen, Screen::Connected(p) if p.queue.waiting_for_capacity.contains(ticket))
    }
    fn implementation_elapsed(&self, ticket: &str) -> Option<String> {
        let Screen::Connected(project) = &self.screen else {
            return None;
        };
        let started = project.activity.tasks.get(ticket)?.telemetry.started_ms?;
        let seconds = (chrono::Utc::now().timestamp_millis() - started).max(0) / 1000;
        Some(format!("{}m {:02}s", seconds / 60, seconds % 60))
    }
    fn auto_plan(&self) -> bool {
        matches!(&self.screen, Screen::Connected(p) if p.queue.auto_plan)
    }
    fn auto_build(&self) -> bool {
        matches!(&self.screen, Screen::Connected(p) if p.queue.auto_build)
    }
    fn auto_implement(&self) -> bool {
        matches!(&self.screen, Screen::Connected(p) if p.queue.auto_build)
    }
    fn auto_publish(&self) -> bool {
        matches!(&self.screen, Screen::Connected(p) if p.queue.auto_publish)
    }
    fn require_independent_checks(&self) -> bool {
        matches!(&self.screen, Screen::Connected(project) if project.queue.require_independent_checks)
    }
    fn queue_status(&self) -> &str {
        match &self.screen {
            Screen::Connected(p) if !p.queue.last_error.is_empty() => &p.queue.last_error,
            Screen::Connected(p) if !p.queue.waiting_for_capacity.is_empty() => {
                "Ready tasks are queued while all implementation slots are occupied"
            }
            Screen::Connected(p) if p.queue.running => "Automatic build queue is running",
            _ => "",
        }
    }
    fn stale_task_claim(&self) -> Option<(String, crate::core::task_claim::ClaimRecord)> {
        match &self.screen {
            Screen::Connected(project) => project.queue.stale_claim.clone(),
            Screen::Welcome => None,
        }
    }
    fn planning_board(&self) -> crate::ui::planning_board::ViewModel {
        self.planning_board_for_surface()
    }
    fn next_question_id(&self) -> Option<&str> {
        match &self.screen {
            Screen::Connected(p) => p.next_question_id.as_deref(),
            Screen::Welcome => None,
        }
    }

    fn spec_text(&self) -> &str {
        match &self.screen {
            Screen::Connected(p) => p
                .live_progress
                .specification
                .as_deref()
                .or(p.state.spec_text.as_deref())
                .unwrap_or(NO_SPEC_PLACEHOLDER),
            Screen::Welcome => "",
        }
    }

    fn active_features(&self) -> Vec<(&str, &str)> {
        match &self.screen {
            Screen::Connected(p) => p
                .state
                .active_features
                .iter()
                .map(|(id, body)| (id.as_str(), body.as_str()))
                .collect(),
            Screen::Welcome => Vec::new(),
        }
    }
    fn feature_approved(&self, id: &str) -> bool {
        match &self.screen {
            Screen::Connected(p) => crate::core::workflow::feature_approved(
                &p.state.planning_store,
                &p.state.workflow,
                id,
            ),
            Screen::Welcome => false,
        }
    }
    fn feature_actions(
        &self,
        conversation: Option<&str>,
    ) -> Vec<crate::ui::feature_approval::Action> {
        self.available_feature_actions(conversation)
    }
    fn toasts(&mut self) -> &mut ToastQueue {
        &mut self.toasts
    }

    details::specification_status::specification_status_surface_methods!();
}
