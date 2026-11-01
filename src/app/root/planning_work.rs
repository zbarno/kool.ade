use super::{KooladeApp, Screen};
use crate::core::planning_work::{Work, WorkKind};

mod generation;

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
    pub(super) fn create_planning_task(
        &mut self,
        kind: WorkKind,
        description: &str,
        parent_uid: Option<String>,
        source_branch: Option<String>,
        destination_branch: Option<String>,
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
            let default_branch = if !selected_code_repository {
                None
            } else if project.git.default_branch.is_empty() {
                Some(if project.git.branch.is_empty() {
                    "main"
                } else {
                    project.git.branch.as_str()
                })
            } else {
                Some(project.git.default_branch.as_str())
            };
            let source_branch = inherited
                .and_then(|work| work.source_branch.clone())
                .or(source_branch)
                .or_else(|| default_branch.map(str::to_owned));
            let destination_branch = inherited
                .and_then(|work| work.destination_branch.clone())
                .or(destination_branch)
                .or_else(|| default_branch.map(str::to_owned));
            let destinations = if !project.git.has_origin {
                &project.git.branches
            } else {
                &project.git.remote_branches
            };
            if selected_code_repository {
                for (label, branch, candidates) in [
                    ("source", &source_branch, &project.git.branches),
                    ("destination", &destination_branch, destinations),
                ] {
                    if let Some(branch) = branch {
                        let default_is_valid = !project.git.has_origin
                            && candidates.is_empty()
                            && (project.git.branch == *branch
                                || default_branch == Some(branch.as_str()));
                        if !candidates.contains(branch) && !default_is_valid {
                            failure = Some(format!(
                                "The selected {label} branch '{branch}' is not present in this repository. Refresh the repository and select an existing branch."
                            ));
                            break;
                        }
                    }
                }
            }
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
