use super::*;
use crate::{harness::AiHarness, persistence::setup_tasks::LocalTask};
use std::sync::atomic::Ordering;

impl Controller {
    pub(in crate::app::root::bootstrap_conversation) fn start(
        &mut self,
        task: &LocalTask,
        host: crate::app::setup_attention::catalog::HostKind,
        harness: Box<dyn AiHarness>,
    ) -> Result<(), String> {
        self.start_context(
            &super::super::Context::Task(Box::new(task.clone())),
            host,
            harness,
        )
    }

    pub(in crate::app::root::bootstrap_conversation) fn start_context(
        &mut self,
        context: &super::super::Context,
        host: crate::app::setup_attention::catalog::HostKind,
        harness: Box<dyn AiHarness>,
    ) -> Result<(), String> {
        self.ensure_loaded(context.id());
        if self.active.contains_key(context.id()) {
            return Err("Wait for this setup-assistant response to finish or cancel it.".into());
        }
        let raw_text = self
            .conversations
            .get(context.id())
            .map(|conversation| conversation.draft.trim().to_owned())
            .unwrap_or_default();
        if raw_text.is_empty() {
            return Err("Write a message before sending it to the setup assistant.".into());
        }
        if raw_text.len() > 8 * 1024 {
            return Err("Setup-assistant messages must be 8 KiB or less.".into());
        }

        let user_message = ChatMessage::new(ChatRole::User, raw_text, None);
        let messages =
            BootstrapConversationStore::append(context.id(), std::slice::from_ref(&user_message))
                .map_err(|_| "The setup conversation could not be saved locally.".to_owned())?;
        let persisted_user = messages
            .iter()
            .find(|message| message.id == user_message.id)
            .cloned()
            .ok_or_else(|| "The setup message could not be stored safely.".to_owned())?;
        {
            let conversation = self
                .conversations
                .entry(context.id().to_owned())
                .or_default();
            conversation.loaded = true;
            conversation.messages = messages.clone();
            conversation.draft.clear();
            conversation.error = None;
            conversation.suggestion = None;
        }
        let history = messages
            .iter()
            .filter(|message| message.id != persisted_user.id)
            .cloned()
            .collect::<Vec<_>>();
        let private_directory = tempfile::Builder::new()
            .prefix("koolade-bootstrap-context-")
            .tempdir()
            .map_err(|_| "A private setup-assistant workspace could not be prepared.".to_owned())?;
        let cancel = Arc::new(AtomicBool::new(false));
        let request = super::super::prompt::render_context_request(
            context,
            &history,
            &persisted_user,
            host,
            private_directory.path().to_path_buf(),
            cancel.clone(),
        )?;
        let route = harness.route_provenance(&request).ok_or_else(|| {
            "Setup assistance needs a selected CLI with current authentication and a verified model. Open Coding Tools, refresh its checks, and try again.".to_owned()
        })?;
        if route.work_category.as_deref() != Some(crate::persistence::harness_settings::PLANNING)
            || route.selected_model.is_none()
            || route.capability_probe_id.is_none()
            || route.capability_generation.is_none()
        {
            return Err("The selected setup-assistant route has no current verified model and capability evidence.".into());
        }

        self.next_generation = self.next_generation.wrapping_add(1).max(1);
        let generation = self.next_generation;
        let (done_tx, result) = mpsc::channel();
        let worker_request = request.clone();
        std::thread::spawn(move || {
            let _private_directory = private_directory;
            let response = super::super::worker::execute(harness.as_ref(), &worker_request);
            let _ = done_tx.send(response);
        });
        self.active.insert(
            context.id().to_owned(),
            ActiveTurn {
                task_id: context.id().to_owned(),
                generation,
                task_fingerprint: context.fingerprint(host),
                route,
                request,
                cancel,
                result,
            },
        );
        Ok(())
    }

    pub(in crate::app::root::bootstrap_conversation) fn active_requests(
        &self,
    ) -> Vec<(String, PlanningRequest)> {
        self.active
            .values()
            .map(|active| (active.task_id.clone(), active.request.clone()))
            .collect()
    }

    pub(in crate::app::root::bootstrap_conversation) fn active_route_matches(
        &self,
        task_id: &str,
        route: Option<&HarnessRouteProvenance>,
    ) -> bool {
        self.active
            .get(task_id)
            .is_none_or(|active| route == Some(&active.route))
    }

    pub(in crate::app::root::bootstrap_conversation) fn cancel(&self, task_id: &str) {
        if let Some(active) = self.active.get(task_id) {
            active.cancel.store(true, Ordering::Release);
        }
    }

    pub(in crate::app::root::bootstrap_conversation) fn take_completed(
        &mut self,
    ) -> Vec<CompletedTurn> {
        let finished = self
            .active
            .iter()
            .filter_map(|(task_id, active)| match active.result.try_recv() {
                Ok(result) => Some((task_id.clone(), result)),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some((task_id.clone(), Err("provider worker stopped".into())))
                }
            })
            .collect::<Vec<_>>();
        finished
            .into_iter()
            .filter_map(|(task_id, result)| {
                self.active
                    .remove(&task_id)
                    .map(|active| CompletedTurn { active, result })
            })
            .collect()
    }

    pub(in crate::app::root::bootstrap_conversation) fn finish(
        &mut self,
        completed: CompletedTurn,
        current_task: Option<&LocalTask>,
        current_route: Option<HarnessRouteProvenance>,
    ) {
        let context = current_task
            .cloned()
            .map(Box::new)
            .map(super::super::Context::Task);
        self.finish_context(completed, context.as_ref(), current_route);
    }

    pub(in crate::app::root::bootstrap_conversation) fn finish_context(
        &mut self,
        completed: CompletedTurn,
        current_context: Option<&super::super::Context>,
        current_route: Option<HarnessRouteProvenance>,
    ) {
        let active = completed.active;
        let Some(context) = current_context.filter(|context| {
            context.id() == active.task_id
                && context.fingerprint(crate::app::setup_attention::catalog::HostKind::current())
                    == active.task_fingerprint
        }) else {
            self.invalidate(&active.task_id, "Setup facts changed while the assistant was responding. Check setup again before using a suggestion.");
            return;
        };
        if current_route.as_ref() != Some(&active.route) {
            self.invalidate(&active.task_id, "The selected assistant or verified model changed. The previous response was discarded; send your message again.");
            return;
        }
        if active.cancel.load(Ordering::Acquire) {
            self.append_system(
                &active.task_id,
                "This setup-assistant response was cancelled and will not resume automatically.",
            );
            return;
        }
        let response = match completed.result {
            Ok(response) => response,
            Err(_) => {
                self.set_error(&active.task_id, "The setup assistant could not complete this response. Check Coding Tools and network access, then try again.".into());
                self.append_system(&active.task_id, "The setup assistant did not return a complete response. No setup action was taken.");
                return;
            }
        };
        let suggestion = response
            .suggestion
            .as_ref()
            .filter(|suggestion| context.permits(suggestion));
        let assistant_message =
            ChatMessage::new(ChatRole::Agent, context.reply(response.reply), None);
        match BootstrapConversationStore::append(&active.task_id, &[assistant_message]) {
            Ok(messages) => {
                let conversation = self
                    .conversations
                    .entry(active.task_id.clone())
                    .or_default();
                conversation.messages = messages;
                conversation.error = response.suggestion.as_ref().filter(|_| suggestion.is_none()).map(|_| {
                    "The assistant suggested an action that is not available for this setup state. No action was taken.".into()
                });
                conversation.suggestion = suggestion.cloned().map(|value| SavedSuggestion {
                    value,
                    generation: active.generation,
                    task_fingerprint: active.task_fingerprint,
                    route: active.route.clone(),
                    request: active.request.clone(),
                });
            }
            Err(_) => self.set_error(
                &active.task_id,
                "The assistant response could not be saved to the machine-local conversation."
                    .into(),
            ),
        }
    }

    fn invalidate(&mut self, task_id: &str, message: &str) {
        let conversation = self.conversations.entry(task_id.to_owned()).or_default();
        conversation.suggestion = None;
        conversation.error = Some(message.to_owned());
        self.append_system(task_id, message);
    }

    pub(in crate::app::root::bootstrap_conversation) fn take_suggestion(
        &mut self,
        task_id: &str,
        generation: u64,
    ) -> Option<SavedSuggestion> {
        let conversation = self.conversations.get_mut(task_id)?;
        if conversation.suggestion.as_ref()?.generation != generation {
            return None;
        }
        conversation.suggestion.take()
    }
}

impl Drop for Controller {
    fn drop(&mut self) {
        for active in self.active.values() {
            active.cancel.store(true, Ordering::Release);
        }
    }
}
