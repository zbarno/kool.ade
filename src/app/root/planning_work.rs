use super::{KooladeApp, Screen};
use crate::core::planning_work::{Work, WorkKind};

mod cycles;
mod generation;
mod git_intent;
mod task_git_intent;

pub(super) fn provider_support_follow_up(
    work: &Work,
) -> Option<crate::core::planning_work::FollowUpTaskOffer> {
    if work.kind != WorkKind::Question {
        return None;
    }
    let request = work.request.to_lowercase();
    let asks_about_hosted_anthropic = request.contains("anthropic")
        && request.contains("hosted")
        && request.contains("pi")
        && (request.contains("why")
            || request.contains("can't")
            || request.contains("cannot")
            || request.contains("support"));
    asks_about_hosted_anthropic.then(|| crate::core::planning_work::FollowUpTaskOffer {
        title: "Add hosted Anthropic support through Pi".into(),
        description: "Plan support for hosted Anthropic through Pi while preserving Kool.ad/e's sandbox security boundaries.".into(),
    })
}

impl KooladeApp {
    pub(super) fn approve_task_git_intent(
        &mut self,
        ticket: &str,
        repository_id: &str,
        selection: crate::core::git_intent::GitIntentSelectionV1,
    ) {
        let result = match &mut self.screen {
            Screen::Connected(project) => {
                task_git_intent::approve(project, ticket, repository_id, selection)
            }
            Screen::Welcome => Err("Connect a project before approving task Git intent.".into()),
        };
        match result {
            Ok(()) => self
                .toasts
                .success("Task Git intent approved and saved locally."),
            Err(error) => self.toasts.warning(error),
        }
    }

    pub(super) fn create_planning_task(
        &mut self,
        kind: WorkKind,
        description: &str,
        parent_uid: Option<String>,
        git_intents: std::collections::BTreeMap<
            String,
            crate::core::git_intent::GitIntentSelectionV1,
        >,
        routing_overrides: std::collections::BTreeMap<
            String,
            crate::persistence::harness_settings::WorkRoute,
        >,
    ) {
        let description = description.trim();
        if description.is_empty() {
            return;
        }
        let mut failure = None;
        let key = if let Screen::Connected(project) = &mut self.screen {
            let inherited = parent_uid
                .as_deref()
                .and_then(|uid| project.planning_work.iter().find(|work| work.uid == uid));
            let inherited_routes = inherited.map(|work| work.routing_overrides.clone());
            let inherited_feature =
                inherited.map(|work| (work.feature_id.clone(), work.feature_uid.clone()));
            let inherited_task_intent_uid =
                inherited.and_then(|work| work.task_intent_root_uid().map(str::to_owned));
            let inherited_git_intents = inherited.map(|work| work.git_intents.clone());
            let routes = inherited_routes.unwrap_or(routing_overrides);
            let referenced_feature = (parent_uid.is_none())
                .then(|| {
                    project
                        .state
                        .active_features
                        .iter()
                        .find_map(|(id, markdown)| {
                            let mentions_id = description
                                .split(|character: char| !character.is_ascii_alphanumeric())
                                .any(|token| token == id);
                            let identity =
                                crate::domain::ArtifactIdentity::from_markdown(markdown).ok()??;
                            mentions_id.then(|| (id.clone(), identity.uid))
                        })
                })
                .flatten();
            if parent_uid.is_none() {
                let settings = crate::persistence::harness_settings::load().0;
                if let Err(error) =
                    super::harness_selection::validate_task_routes(&settings, &routes)
                {
                    failure = Some(error);
                }
            }
            let selected_code_repository = project.has_selected_code_repository();
            let git_intents =
                git_intent::inherit_selections(inherited_git_intents.as_ref(), git_intents);
            let requires_git_intent = matches!(
                kind,
                WorkKind::Feature
                    | WorkKind::Bug
                    | WorkKind::NewProject
                    | WorkKind::DocumentationRefresh
            );
            if selected_code_repository && requires_git_intent && git_intents.is_empty() {
                failure = Some(
                    "Choose at least one code repository and approve its source, destination, and publication mode before creating this task.".into(),
                );
            }
            let mut approved_git_intents = std::collections::BTreeMap::new();
            if requires_git_intent && selected_code_repository {
                for (repository_id, selection) in &git_intents {
                    if let Err(error) = git_intent::validate_selection(
                        project,
                        repository_id,
                        &selection.source_ref,
                        &selection.destination_ref,
                    ) {
                        failure = Some(format!(
                            "Cannot approve Git intent for {repository_id}: {error}"
                        ));
                        break;
                    }
                    let inherited = inherited_git_intents
                        .as_ref()
                        .and_then(|intents| intents.get(repository_id));
                    match git_intent::approve_for_work(
                        project,
                        repository_id,
                        &selection.source_ref,
                        &selection.destination_ref,
                        selection.publication_mode,
                        inherited,
                        parent_uid.clone(),
                    ) {
                        Ok(intent) => {
                            approved_git_intents.insert(repository_id.clone(), intent);
                        }
                        Err(error) => {
                            failure = Some(format!(
                                "Cannot approve Git intent for {repository_id}: {error}"
                            ));
                            break;
                        }
                    }
                }
            }
            let source_branch = approved_git_intents
                .get("root")
                .map(|intent| intent.source_ref.clone());
            let destination_branch = approved_git_intents
                .get("root")
                .map(|intent| intent.destination_ref.clone());
            let approved_git_intent = approved_git_intents.get("root").cloned();
            if failure.is_some() {
                None
            } else {
                let prior_offer = if let Some(parent_uid) = &parent_uid {
                    project
                        .planning_work
                        .iter_mut()
                        .find(|work| &work.uid == parent_uid && work.follow_up_task.is_some())
                        .and_then(|parent| parent.follow_up_task.take())
                } else {
                    None
                };
                if parent_uid.is_some() && prior_offer.is_none() {
                    failure = Some("This related task offer is no longer available.".into());
                    None
                } else {
                    let mut work = Work::new(
                        String::new(),
                        format!(
                            "{}: {}",
                            kind.label(),
                            crate::core::context_build::clip(description, 72)
                        ),
                        description.to_owned(),
                        "Building project context before planning".into(),
                    );
                    work.kind = kind;
                    work.parent_uid = parent_uid.clone();
                    work.task_intent_uid = inherited_task_intent_uid
                        .or_else(|| kind.starts_task_intent().then(|| work.uid.clone()));
                    work.feature_id = inherited_feature
                        .as_ref()
                        .and_then(|(feature_id, _)| feature_id.clone())
                        .or_else(|| {
                            referenced_feature
                                .as_ref()
                                .map(|(feature_id, _)| feature_id.clone())
                        });
                    work.feature_uid = inherited_feature
                        .and_then(|(_, feature_uid)| feature_uid)
                        .or_else(|| referenced_feature.map(|(_, feature_uid)| feature_uid));
                    work.routing_overrides = routes;
                    work.routing_inherited_from = parent_uid
                        .as_ref()
                        .filter(|_| !work.routing_overrides.is_empty())
                        .cloned();
                    work.source_branch = source_branch;
                    work.destination_branch = destination_branch;
                    work.git_intent = approved_git_intent;
                    work.git_intents = approved_git_intents;
                    work.key = format!("task:{}", work.uid);
                    let key = work.key.clone();
                    project.planning_work.push(work);
                    if let Err(error) = project.save_planning_work() {
                        project.planning_work.pop();
                        if let (Some(parent_uid), Some(offer)) = (&parent_uid, prior_offer)
                            && let Some(parent) = project
                                .planning_work
                                .iter_mut()
                                .find(|work| &work.uid == parent_uid)
                        {
                            parent.follow_up_task = Some(offer);
                        }
                        failure = Some(format!("Cannot save the new task: {error}"));
                        None
                    } else {
                        Some(key)
                    }
                }
            }
        } else {
            failure = Some("Connect a project before creating a task.".into());
            None
        };
        if let Some(message) = failure {
            self.toasts.warning(message);
            return;
        }
        let Some(key) = key else { return };
        if let Screen::Connected(project) = &mut self.screen {
            project
                .task_chats
                .drafts
                .insert(key.clone(), description.into());
        }
        self.submit_task_reply(&key);
        if matches!(&self.screen, Screen::Connected(project) if project.task_turns.contains_key(&key))
        {
            self.toasts.info(format!(
                "{} task created; independent planning started",
                kind.label()
            ));
        }
    }
}

#[cfg(test)]
#[path = "planning_work/tests.rs"]
mod tests;
