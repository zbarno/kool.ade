use super::*;

impl KooladeApp {
    pub(super) fn start_specification_chat(
        &mut self,
        mut request: crate::core::specification_chat::Request,
    ) {
        if request.text.trim().is_empty() {
            return;
        }
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        if project.active_turn.is_some() {
            return;
        }
        let provenance = request.conversation_context();
        let (recent_chat, omitted_history_messages) =
            project.recent_specification_chat(&provenance.scope_key, 12);
        request.omitted_history_messages = omitted_history_messages;
        let mut user_message = ChatMessage::new(ChatRole::User, &request.text, None);
        user_message.specification_context = Some(provenance);
        project.remember_chat(vec![user_message]);
        project.draft.clear();
        project.task_chats.active = None;
        project.active_planning_work = None;
        project.activity.manager = None;
        if let Err(error) =
            crate::core::specification_chat::validate_target(&project.state, &request.target)
        {
            project.remember_chat(vec![ChatMessage::new(
                ChatRole::System,
                error.to_string(),
                None,
            )]);
            self.toasts.warning(error.to_string());
            return;
        }
        let purpose = match request.intent {
            crate::core::specification_chat::Intent::Ask => {
                crate::core::workflow::TurnPurpose::Question
            }
            crate::core::specification_chat::Intent::Propose => {
                crate::core::workflow::TurnPurpose::Interview
            }
        };
        let inputs = crate::core::turn::TurnInputs {
            state: project.state.clone(),
            user_message: request.text.clone(),
            recent_chat,
            purpose,
            comparison_feature: None,
            specification_chat: Some(request.clone()),
        };
        let harness = crate::app::root::configured_harness_for_project_task(
            &mut self.task_harness,
            None,
            &project.state.harness_routes.routes,
            &Default::default(),
        );
        let route_label = harness.label();
        let controller = match request.intent {
            crate::core::specification_chat::Intent::Ask => {
                TurnController::start_read_only(inputs, harness, None)
            }
            crate::core::specification_chat::Intent::Propose => {
                TurnController::start(inputs, harness)
            }
        };
        project.active_turn = Some(std::rc::Rc::new(controller));
        let activity = match request.intent {
            crate::core::specification_chat::Intent::Ask => {
                "Answering from the pinned specification"
            }
            crate::core::specification_chat::Intent::Propose => {
                "Preparing a private specification draft"
            }
        };
        project.live_progress = crate::harness::LiveProgress {
            activity: Some(format!("{activity} via {route_label}")),
            selected_route: Some(route_label),
            ..Default::default()
        };
    }
}
