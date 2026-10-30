use super::{Dialog, KooladeApp};
use crate::{
    harness::{
        BootstrapConversationReplyV1, BootstrapImageProfileV1, BootstrapIntentOperationV1,
        BootstrapIntentSuggestionV1,
    },
    persistence::setup_tasks::LocalTask,
    ui::planning_board::BootstrapSuggestionOperation,
};
use std::collections::BTreeMap;

mod controller;
mod prompt;
#[cfg(test)]
mod tests;
mod worker;

pub(crate) use controller::Controller;

impl KooladeApp {
    pub(super) fn send_bootstrap_conversation_message(&mut self, task_id: &str) {
        let Some(task) = self
            .machine_setup_tasks
            .iter()
            .find(|task| task.id == task_id)
            .cloned()
        else {
            self.toasts
                .warning("This setup task is no longer available. Check the machine again.");
            return;
        };
        let mut override_harness = self.task_harness.take();
        let harness = super::harness_selection::configured_bootstrap_harness(&mut override_harness);
        match self.bootstrap_conversation.start(
            &task,
            crate::app::setup_attention::catalog::HostKind::current(),
            harness,
        ) {
            Ok(()) => {}
            Err(error) => {
                self.bootstrap_conversation
                    .set_error(task_id, error.clone());
                self.toasts.warning(error);
            }
        }
    }

    pub(super) fn poll_bootstrap_conversation(&mut self) {
        let requests = self.bootstrap_conversation.active_requests();
        if requests.is_empty() {
            return;
        }
        let mut no_override = None;
        let selected = super::harness_selection::configured_bootstrap_harness(&mut no_override);
        let mut current_routes = BTreeMap::new();
        for (task_id, request) in requests {
            let current_route = selected.route_provenance(&request);
            if !self
                .bootstrap_conversation
                .active_route_matches(&task_id, current_route.as_ref())
            {
                self.bootstrap_conversation.cancel(&task_id);
            }
            current_routes.insert(task_id, current_route);
        }
        for completed in self.bootstrap_conversation.take_completed() {
            let task_id = completed.task_id().to_owned();
            let task = self
                .machine_setup_tasks
                .iter()
                .find(|task| task.id == task_id);
            self.bootstrap_conversation.finish(
                completed,
                task,
                current_routes.remove(&task_id).flatten(),
            );
        }
    }

    pub(super) fn cancel_bootstrap_conversation(&mut self, task_id: &str) {
        self.bootstrap_conversation.cancel(task_id);
    }

    pub(super) fn apply_bootstrap_suggestion(&mut self, task_id: &str, generation: u64) {
        let Some(task) = self
            .machine_setup_tasks
            .iter()
            .find(|task| task.id == task_id)
            .cloned()
        else {
            self.bootstrap_conversation.set_error(
                task_id,
                "This setup task changed. Check the machine again before continuing.".into(),
            );
            return;
        };
        let Some(suggestion) = self
            .bootstrap_conversation
            .take_suggestion(task_id, generation)
        else {
            self.toasts
                .warning("This setup suggestion expired. Ask again using the current setup facts.");
            return;
        };
        if suggestion.task_fingerprint
            != task_fingerprint(
                &task,
                crate::app::setup_attention::catalog::HostKind::current(),
            )
            || !suggest_remediation_intent(&task, &suggestion.value)
        {
            self.bootstrap_conversation.set_error(
                task_id,
                "Setup facts changed, so this suggestion is no longer available. Check setup again before continuing.".into(),
            );
            return;
        }
        let mut no_override = None;
        let selected = super::harness_selection::configured_bootstrap_harness(&mut no_override);
        if selected.route_provenance(&suggestion.request).as_ref() != Some(&suggestion.route) {
            self.bootstrap_conversation.set_error(
                task_id,
                "The selected assistant or verified model changed. Ask again before using this suggestion.".into(),
            );
            self.toasts
                .warning("This setup suggestion expired after the assistant route changed.");
            return;
        }
        match suggestion.value.operation {
            BootstrapIntentOperationV1::RecheckPrerequisites => self.refresh_setup_attention(true),
            BootstrapIntentOperationV1::PrepareLockedImage => self.request_setup_task_fix(task_id),
            BootstrapIntentOperationV1::OpenCodingTools => {
                self.dialog = Some(Dialog::HarnessSetup(
                    crate::app::dialogs::DlgHarnessSetup::new(),
                ));
            }
        }
    }
}

fn suggestion_view(suggestion: &BootstrapIntentSuggestionV1) -> BootstrapSuggestionOperation {
    match suggestion.operation {
        BootstrapIntentOperationV1::RecheckPrerequisites => {
            BootstrapSuggestionOperation::RecheckPrerequisites
        }
        BootstrapIntentOperationV1::PrepareLockedImage => {
            BootstrapSuggestionOperation::PrepareLockedImage {
                profile: match suggestion.image_profile {
                    Some(BootstrapImageProfileV1::General) => "general",
                    Some(BootstrapImageProfileV1::Rust) => "rust",
                    Some(BootstrapImageProfileV1::Node) => "node",
                    Some(BootstrapImageProfileV1::Python) => "python",
                    Some(BootstrapImageProfileV1::Go) => "go",
                    None => "",
                }
                .to_owned(),
            }
        }
        BootstrapIntentOperationV1::OpenCodingTools => {
            BootstrapSuggestionOperation::OpenCodingTools
        }
    }
}

fn response_message(reply: BootstrapConversationReplyV1, task: &LocalTask) -> String {
    let bounded = |text: &str| text.chars().take(700).collect::<String>();
    match reply {
        BootstrapConversationReplyV1::ExplainObservation => format!(
            "The setup task's current observation is: {}",
            bounded(&task.explanation)
        ),
        BootstrapConversationReplyV1::RepeatNextStep => format!(
            "The setup task's suggested next step is: {}",
            bounded(&task.next_step)
        ),
        BootstrapConversationReplyV1::AskWhatToExplain => {
            "Which part of this setup task should I explain?".into()
        }
        BootstrapConversationReplyV1::AskWhatTaskShows => {
            "What does the setup task show after the latest check?".into()
        }
    }
}

fn suggest_remediation_intent(task: &LocalTask, suggestion: &BootstrapIntentSuggestionV1) -> bool {
    use crate::persistence::setup_tasks::State;
    match suggestion.operation {
        BootstrapIntentOperationV1::RecheckPrerequisites => {
            !matches!(task.state, State::Verified | State::Superseded)
        }
        BootstrapIntentOperationV1::OpenCodingTools => {
            task.id.starts_with("setup:cli:") && task.state != State::Verified
        }
        BootstrapIntentOperationV1::PrepareLockedImage => {
            let expected_profile =
                crate::app::bootstrap_repair_policy::parse_locked_image_task_id(&task.id)
                    .map(|(profile, _, _)| profile);
            let suggested_profile = match suggestion.image_profile {
                Some(BootstrapImageProfileV1::General) => Some("general"),
                Some(BootstrapImageProfileV1::Rust) => Some("rust"),
                Some(BootstrapImageProfileV1::Node) => Some("node"),
                Some(BootstrapImageProfileV1::Python) => Some("python"),
                Some(BootstrapImageProfileV1::Go) => Some("go"),
                None => None,
            };
            expected_profile == suggested_profile
                && task.blocked_by.is_empty()
                && matches!(
                    task.state,
                    State::ReadyToFix
                        | State::Failed
                        | State::Paused
                        | State::Interrupted
                        | State::Cancelled
                )
        }
    }
}

fn task_fingerprint(
    task: &LocalTask,
    host: crate::app::setup_attention::catalog::HostKind,
) -> String {
    use sha2::Digest as _;
    let stable = serde_json::json!({
        "id": &task.id,
        "title": &task.title,
        "explanation": &task.explanation,
        "nextStep": &task.next_step,
        "platform": host,
        "state": task.state,
        "scanGeneration": task.scan_generation,
        "blockedBy": &task.blocked_by,
        "verifiedResult": &task.verified_result,
    });
    format!(
        "{:x}",
        sha2::Sha256::digest(serde_json::to_vec(&stable).unwrap_or_default())
    )
}
