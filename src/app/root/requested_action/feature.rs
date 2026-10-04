use super::*;

pub(super) fn approve(app: &mut KooladeApp, target: Option<String>) {
    let item = target.unwrap_or_else(|| "the feature".into());
    action_feedback(
        app,
        &format!(
            "Open {item}'s review card on the Kanban to read the current specification and approve it."
        ),
    );
}

pub(super) fn generate(app: &mut KooladeApp, target: Option<String>) {
    let item = target.unwrap_or_else(|| "the approved feature".into());
    action_feedback(
        app,
        &format!("Use {item}'s Generate tasks card on the Kanban."),
    );
}

use super::action_feedback;
