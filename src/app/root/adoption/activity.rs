use super::*;

impl KooladeApp {
    /// Derived caches read by other layers through `Surface`.
    pub(in crate::app::root) fn refresh_derived(&mut self, project: &Project) {
        (self.cached_user, self.synth) = Self::derive_caches(project);
    }

    /// Pure derivation of the display caches, kept separated from `&mut
    /// self` so `tick` can compute them without double-borrowing.
    ///
    /// The cached routing identity is THE SEATED OPERATOR (FR-13), so the
    /// panel highlight, the chat eligibility display, and the turn pipeline
    /// all judge one and the same identity.
    pub(in crate::app::root) fn derive_caches(project: &Project) -> (CurrentUser, Vec<OpenItem>) {
        (
            project.state.effective_user(),
            crate::core::ownership::synthesize_for_state(&project.state),
        )
    }

    pub(super) fn reset_turn_activity(&mut self, project: &mut Project) {
        project.activity.ensure_overall();
        project
            .activity
            .overall
            .as_mut()
            .unwrap()
            .update(Default::default());
        let activity_key = project
            .task_chats
            .active
            .clone()
            .unwrap_or_else(|| "__main".into());
        project
            .activity
            .conversations
            .entry(activity_key)
            .or_default()
            .update(Default::default());
    }
}
