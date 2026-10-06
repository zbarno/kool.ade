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
    ) {
        let description = description.trim();
        if description.is_empty() {
            return;
        }
        let mut failure = None;
        let key = if let Screen::Connected(project) = &mut self.screen {
            if project.active_turn.is_some() {
                failure = Some(
                    "Finish the current planning turn before creating another task.".to_owned(),
                );
                None
            } else {
                let inherited = parent_uid
                    .as_deref()
                    .and_then(|uid| project.planning_work.iter().find(|work| work.uid == uid));
                let default_branch = if project.git.default_branch.is_empty() {
                    if project.git.branch.is_empty() {
                        "main"
                    } else {
                        project.git.branch.as_str()
                    }
                } else {
                    project.git.default_branch.as_str()
                };
                let source_branch = inherited
                    .and_then(|work| work.source_branch.clone())
                    .or(source_branch)
                    .unwrap_or_else(|| default_branch.to_owned());
                let destination_branch = inherited
                    .and_then(|work| work.destination_branch.clone())
                    .or(destination_branch)
                    .unwrap_or_else(|| default_branch.to_owned());
                let destinations = if !project.git.has_origin {
                    &project.git.branches
                } else {
                    &project.git.remote_branches
                };
                for (label, branch, candidates) in [
                    ("source", &source_branch, &project.git.branches),
                    ("destination", &destination_branch, destinations),
                ] {
                    let default_is_valid = !project.git.has_origin
                        && candidates.is_empty()
                        && (project.git.branch == *branch || default_branch == branch.as_str());
                    if !candidates.contains(branch) && !default_is_valid {
                        failure = Some(format!(
                            "The selected {label} branch '{branch}' is not present in this repository. Refresh the repository and select an existing branch."
                        ));
                        break;
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
                        work.source_branch = Some(source_branch);
                        work.destination_branch = Some(destination_branch);
                        work.key = format!("task:{}", work.uid);
                        let key = work.key.clone();
                        project.planning_work.push(work);
                        if let Err(error) = crate::core::planning_work::save(
                            &project.state.repo_root,
                            &project.planning_work,
                        ) {
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
        let purpose = if kind == WorkKind::Question {
            crate::core::workflow::TurnPurpose::Question
        } else {
            crate::core::workflow::TurnPurpose::Interview
        };
        self.start_turn_for_work(description, purpose, None, Some(key));
        self.toasts
            .info(format!("{} task created; planning started", kind.label()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn work(kind: WorkKind, request: &str) -> Work {
        let mut work = Work::new(
            String::new(),
            "Question".into(),
            request.into(),
            String::new(),
        );
        work.kind = kind;
        work
    }

    #[test]
    fn unsupported_hosted_anthropic_question_gets_an_optional_offer_only() {
        let offer = provider_support_follow_up(&work(
            WorkKind::Question,
            "Why can't Kool.ad/e use hosted Anthropic through Pi?",
        ))
        .unwrap();
        assert!(offer.title.contains("hosted Anthropic"));
        assert!(offer.description.contains("sandbox security boundaries"));
        assert!(
            provider_support_follow_up(&work(
                WorkKind::Question,
                "How does Anthropic compare with local providers?",
            ))
            .is_none()
        );
        assert!(
            provider_support_follow_up(&work(
                WorkKind::Feature,
                "Why can't Kool.ad/e use hosted Anthropic through Pi?",
            ))
            .is_none()
        );
    }
}
