use super::Project;

impl Project {
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
