use super::Project;

impl Project {
    pub fn persist_manager_run_event(
        &mut self,
        event: crate::domain::ManagerRunEventV1,
        text: impl Into<String>,
    ) -> Result<(), String> {
        let message = crate::domain::ChatMessage::new(crate::domain::ChatRole::System, text, None);
        self.remember_manager_run_message(message, event)
    }

    pub fn remember_manager_run_message(
        &mut self,
        mut message: crate::domain::ChatMessage,
        event: crate::domain::ManagerRunEventV1,
    ) -> Result<(), String> {
        let previous = self
            .activity
            .manager_history
            .iter()
            .rev()
            .filter_map(|message| message.manager_run_event.as_ref())
            .find(|previous| previous.run_id == event.run_id);
        event
            .validate_after(previous)
            .map_err(|error| error.to_string())?;
        message.manager_run_event = Some(event);
        crate::persistence::manager_activity::append(
            &self.chat_slug,
            std::slice::from_ref(&message),
        )
        .map_err(|error| error.to_string())?;
        self.activity.manager_history.push(message);
        if self.activity.manager_history.len() > 300 {
            self.activity.manager_history.remove(0);
        }
        self.activity.manager_history_error = None;
        Ok(())
    }

    pub fn remember_manager_activity(&mut self, message: crate::domain::ChatMessage) {
        self.activity.manager_history.push(message.clone());
        if self.activity.manager_history.len() > 300 {
            self.activity.manager_history.remove(0);
        }
        if let Err(error) = crate::persistence::manager_activity::append(
            &self.chat_slug,
            std::slice::from_ref(&message),
        ) {
            self.activity.manager_history_error = Some(format!(
                "Manager activity is visible for this session but could not be saved: {error}"
            ));
        } else {
            self.activity.manager_history_error = None;
        }
    }
}
