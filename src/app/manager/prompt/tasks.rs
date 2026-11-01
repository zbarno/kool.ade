use super::super::super::session::Project;

pub(super) fn conversation_reply_pending(project: &Project, key: &str) -> bool {
    project.task_turns.contains_key(key) || project.task_chats.has_pending_user_reply(key)
}

pub(super) fn active_worker_progress(project: &Project) -> Vec<String> {
    project
        .active_implementations
        .keys()
        .take(8)
        .map(|ticket| {
            let title = project
                .task_documents
                .iter()
                .find(|document| &document.path == ticket)
                .map(|document| document.title.as_str())
                .unwrap_or(ticket);
            let activity = project
                .activity
                .tasks
                .get(ticket)
                .and_then(|progress| progress.activity.as_deref());
            format!("{title}: {}", progress_label(activity))
        })
        .collect()
}

pub(super) fn progress_label(activity: Option<&str>) -> &'static str {
    let Some(activity) = activity else {
        return "active; no current progress signal";
    };
    if activity.starts_with("Verifying:") {
        "running required verification"
    } else if activity.starts_with("Refreshing public NuGet vulnerability data") {
        "refreshing the public NuGet audit feed before retrying verification"
    } else if activity.starts_with("koolade_bash") {
        "running a sandbox command"
    } else if activity.starts_with("koolade_resource") {
        "requesting a sandbox resource"
    } else if activity.contains("reconcil") || activity.contains("merge") {
        "reconciling repository history"
    } else if activity.starts_with("Preparing implementation") {
        "preparing the task repository"
    } else {
        "active; latest progress is available in task activity"
    }
}

pub(super) fn planning_context(
    work: &[crate::core::planning_work::Work],
    active_turn: Option<&str>,
) -> String {
    let items = work
        .iter()
        .rev()
        .take(8)
        .map(|work| {
            format!(
                "{}: {:?} — {}",
                work.title,
                work.status,
                crate::core::context_build::clip(&work.detail, 160)
            )
        })
        .collect::<Vec<_>>();
    format!(
        "Active planning turn: {active_turn:?}\nInternal planning work (status alone does not mean the operator owes an answer): {items:?}"
    )
}

pub(super) fn summarize_task(
    project: &Project,
    doc: &crate::artifacts::task_docs::TaskDocument,
    waiting_on_user: &mut Vec<String>,
    saved_replies_pending: &mut Vec<String>,
    internal_recovery: &mut Vec<String>,
    blocked_elsewhere: &mut Vec<String>,
) -> String {
    if let Some(dependency) = unresolved_dependency(project, doc) {
        blocked_elsewhere.push(format!(
            "{} waits for prerequisite {}",
            doc.title, dependency
        ));
    }
    let state = project.implementation_states.get(&doc.path);
    let active = project.active_implementations.contains_key(&doc.path);
    let status = if active {
        Some(crate::core::implementation::ImplementationStatus::Preparing)
    } else {
        state.map(|record| record.status)
    };
    let pull_request_closed = state.is_some_and(|record| {
        record.pr_state == Some(crate::core::implementation::PullRequestState::Closed)
    });
    if project.task_chats.has_failed_task_reply(&doc.path) {
        internal_recovery.push(format!(
            "{}: a submitted answer failed during internal processing; recover from its saved conversation without asking the user to repeat it",
            doc.title
        ));
        return format!(
            "{}: {} — internal reply recovery required",
            doc.path, doc.title
        );
    }
    let reply_pending = conversation_reply_pending(project, &doc.path);
    if reply_pending {
        saved_replies_pending.push(format!(
            "{}: saved user reply awaits internal processing",
            doc.title
        ));
    }
    if pull_request_closed && !reply_pending {
        waiting_on_user.push(format!("{}: Pull request closed", doc.title));
    }
    if !active && let Some(failure) = project.queue.blocked.get(&doc.path) {
        let summary =
            crate::core::context_build::clip(&format!("{}: {}", doc.title, failure.message), 400);
        match failure.recovery {
            crate::core::implementation::RecoveryDisposition::UserAction
            | crate::core::implementation::RecoveryDisposition::ExplicitResume => {
                if !reply_pending {
                    waiting_on_user.push(summary);
                }
            }
            crate::core::implementation::RecoveryDisposition::AutomaticRetry
            | crate::core::implementation::RecoveryDisposition::DoNotRetry => {
                blocked_elsewhere.push(summary);
            }
        }
        return format!("{}: {} — Needs attention", doc.path, doc.title);
    }

    if !pull_request_closed {
        match status {
            Some(
                current @ (crate::core::implementation::ImplementationStatus::WaitingToMerge
                | crate::core::implementation::ImplementationStatus::WaitingForIndependentChecks),
            ) => blocked_elsewhere.push(format!("{}: {}", doc.title, current.label())),
            Some(
                current @ (crate::core::implementation::ImplementationStatus::ReadyToPublish
                | crate::core::implementation::ImplementationStatus::AwaitingApproval
                | crate::core::implementation::ImplementationStatus::ChangesRequested
                | crate::core::implementation::ImplementationStatus::PullRequestClosed
                | crate::core::implementation::ImplementationStatus::Interrupted),
            ) => {
                if !reply_pending {
                    waiting_on_user.push(format!("{}: {}", doc.title, current.label()));
                }
            }
            Some(crate::core::implementation::ImplementationStatus::Blocked) => {
                blocked_elsewhere.push(format!("{}: Needs attention", doc.title));
            }
            _ => {}
        }
    }
    format!(
        "{}: {} — {}",
        doc.path,
        doc.title,
        status.map(|current| current.label()).unwrap_or("To do")
    )
}

fn unresolved_dependency(
    project: &Project,
    doc: &crate::artifacts::task_docs::TaskDocument,
) -> Option<String> {
    let metadata = doc.metadata.as_ref()?;
    for uid in &metadata.dependency_uids {
        let dependency = project.task_documents.iter().find(|candidate| {
            candidate
                .identity
                .as_ref()
                .is_some_and(|identity| &identity.uid == uid)
        });
        let Some(dependency) = dependency else {
            return Some("a missing task prerequisite".into());
        };
        let complete = project
            .implementation_states
            .get(&dependency.path)
            .is_some_and(|state| {
                state.status == crate::core::implementation::ImplementationStatus::Completed
                    || state.pr_state == Some(crate::core::implementation::PullRequestState::Merged)
            });
        if !complete {
            return Some(dependency.title.clone());
        }
    }
    None
}
