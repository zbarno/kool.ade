use super::*;

impl KooladeApp {
    pub(in crate::app::root) fn submit_task_reply(&mut self, key: &str) {
        self.submit_task_reply_mode(key, false);
    }

    pub(in crate::app::root) fn submit_task_reply_mode(&mut self, key: &str, read_only: bool) {
        let review_changes = !read_only
            && matches!(&self.screen, Screen::Connected(project)
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
        let work = crate::core::planning_work::find(&project.state, key);
        let work_type = work.as_ref().map(|work| work.kind.harness_route_category());
        let task_routes = work
            .as_ref()
            .map(|work| work.routing_overrides.clone())
            .unwrap_or_default();
        let harness = crate::app::root::configured_harness_for_project_task(
            &mut self.task_harness,
            work_type,
            &project.state.harness_routes.routes,
            &task_routes,
        );
        if let Some(blocker) = harness.route_blocker() {
            self.toasts.warning(format!(
                "Reply not sent; your draft is preserved. {blocker}"
            ));
            return;
        }
        project.bind_task_conversation_identities();
        project.task_chats.ensure_loaded(&project.chat_slug);
        let mut user_message = ChatMessage::new(ChatRole::User, &text, Some(key.into()));
        user_message.clarification = read_only;
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
        if !read_only {
            super::documentation_refresh::record(project, work.as_ref(), &sent_id);
        }
        let purpose = if crate::core::task_conversation::is_opportunity_conversation_key(key)
            || work
                .as_ref()
                .is_some_and(|work| work.kind == crate::core::planning_work::WorkKind::Question)
        {
            crate::core::workflow::TurnPurpose::Question
        } else {
            crate::core::workflow::TurnPurpose::Interview
        };
        let inputs = crate::core::turn::TurnInputs {
            state: project.state.clone(),
            user_message: text,
            recent_chat,
            purpose,
            comparison_feature: None,
        };
        project.task_chats.drafts.remove(key);
        let route_label = harness.label();
        let start = if read_only {
            TurnController::start_read_only
        } else {
            TurnController::start_scoped
        };
        project.task_turns.insert(
            key.into(),
            std::rc::Rc::new(start(inputs, harness, Some(key.into()))),
        );
        project.task_live.insert(
            key.into(),
            crate::harness::LiveProgress {
                selected_route: Some(route_label),
                ..Default::default()
            },
        );
    }
}
