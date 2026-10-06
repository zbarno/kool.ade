use super::*;

mod details;

impl Surface for KooladeApp {
    fn session_title(&self) -> &str {
        match &self.screen {
            Screen::Connected(p) => p.state.title.as_str(),
            Screen::Welcome => "",
        }
    }

    fn is_git_repo(&self) -> bool {
        matches!(&self.screen, Screen::Connected(_))
    }

    fn registered_repositories(&self) -> Vec<crate::ui::RepositoryChoice> {
        repository_switcher::choices(&self.screen)
    }

    fn git_branch(&self) -> &str {
        match &self.screen {
            Screen::Connected(p) => p.git.branch.as_str(),
            Screen::Welcome => "",
        }
    }

    fn repository_branches(&self) -> Vec<String> {
        match &self.screen {
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
            Screen::Connected(project) if !project.git.has_origin => self.repository_branches(),
            Screen::Connected(project) => project.git.remote_branches.clone(),
            Screen::Welcome => Vec::new(),
        }
    }

    fn default_repository_branch(&self) -> &str {
        match &self.screen {
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
            Screen::Connected(p) => p.git.head_short.as_str(),
            Screen::Welcome => "",
        }
    }

    fn git_dirty(&self) -> bool {
        match &self.screen {
            Screen::Connected(p) => p.git.dirty > 0,
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
            Screen::Welcome => &mut self.conn_path,
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
                && matches!(crate::core::implementation_queue::next_ticket(
                    &p.task_documents, &p.implementation_states), Ok(Some(_))))
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
            Screen::Connected(p) => {
                crate::core::workflow::feature_approved(&p.state.repo_root, &p.state.workflow, id)
            }
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
}
