#[derive(Default)]
pub(super) struct AppState {
    clicked: bool,
    typed: String,
}

impl AppState {
    pub(super) fn title(&self) -> String {
        if !self.typed.is_empty() {
            format!("Target:Typed:{}", self.typed)
        } else if self.clicked {
            "Target:Clicked".into()
        } else {
            "Target:Ready".into()
        }
    }

    pub(super) fn click(&mut self) {
        self.clicked = true;
    }

    pub(super) fn type_text(&mut self, text: &str) {
        self.typed
            .extend(text.chars().filter(char::is_ascii_alphanumeric));
    }
}

#[cfg(test)]
mod tests {
    use super::AppState;

    #[test]
    fn native_input_updates_the_visible_window_state() {
        let mut state = AppState::default();
        assert_eq!(state.title(), "Target:Ready");
        state.click();
        assert_eq!(state.title(), "Target:Clicked");
        state.type_text("ok");
        assert_eq!(state.title(), "Target:Typed:ok");
    }
}
