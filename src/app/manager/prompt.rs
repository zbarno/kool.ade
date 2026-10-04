use super::super::session::Project;

pub(super) fn build(project: &Project, events: &[String]) -> String {
    let user = project.state.effective_user();
    let eligible = crate::core::routing::eligible_items(
        &project.state.items,
        &user,
        &project.state.config.stakeholders,
    );
    let mut waiting_on_user = eligible
        .iter()
        .filter(|item| {
            matches!(
                item.authority,
                crate::domain::Authority::Human | crate::domain::Authority::Review
            ) || item.is_ownership_gap()
        })
        .map(|item| {
            crate::core::context_build::clip(
                &format!("{}: {} ({})", item.id, item.question, item.reason),
                400,
            )
        })
        .collect::<Vec<_>>();
    let mut blocked_elsewhere = project
        .state
        .items
        .iter()
        .filter(|item| {
            item.status == crate::domain::ItemStatus::Open && !item.blocked_by.is_empty()
        })
        .map(|item| format!("{} waits for {}", item.id, item.blocked_by.join(", ")))
        .collect::<Vec<_>>();

    for work in project
        .planning_work
        .iter()
        .filter(|work| work.status == crate::core::planning_work::WorkStatus::NeedsAttention)
    {
        waiting_on_user.push(crate::core::context_build::clip(
            &format!("{}: {}", work.title, work.detail),
            400,
        ));
    }

    let mut tasks = project
        .task_documents
        .iter()
        .filter(|doc| !doc.path.ends_with("/README.md"))
        .map(|doc| summarize_task(project, doc, &mut waiting_on_user, &mut blocked_elsewhere))
        .collect::<Vec<_>>();
    tasks.reverse();
    tasks.truncate(8);
    let active_planning_turn = project
        .active_turn
        .as_ref()
        .and(project.active_planning_work.as_deref());
    let planning_work = planning_context(&project.planning_work, active_planning_turn);
    let waiting_on_user = waiting_on_user.into_iter().take(12).collect::<Vec<_>>();
    let blocked_elsewhere = blocked_elsewhere.into_iter().take(12).collect::<Vec<_>>();
    let update = crate::core::context_build::clip(
        &format!(
            "PROJECT MANAGER UPDATE\nProject: {}\nEvents: {:?}\nTask count: {}\nBoard tasks: {:?}\n{}\nActive worker: {:?}\nQueue running: {}\nWaiting on user: {:?}\nBlocked on external work or dependencies: {:?}",
            project.state.title,
            events.iter().rev().take(8).collect::<Vec<_>>(),
            project.task_documents.len(),
            tasks,
            planning_work,
            project.active_implementations.keys().collect::<Vec<_>>(),
            project.queue.running,
            waiting_on_user,
            blocked_elsewhere
        ),
        12000,
    );
    let features = project
        .state
        .active_features
        .iter()
        .map(|(id, text)| format!("{id}\n{}", crate::core::workflow::feature_contract(text)))
        .collect::<Vec<_>>()
        .join("\n\n");
    format!(
        "{update}\n\n{}\n\n=== CURRENT FEATURE CONTRACTS (authoritative over chat history) ===\n{}\n\n{}",
        crate::core::prompt::workflow_context(
            &project.state,
            crate::core::workflow::TurnPurpose::Interview
        ),
        crate::core::context_build::clip(&features, 16000),
        project.task_interaction_context(&events.join("\n"))
    )
}

fn planning_context(
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
    format!("Active planning turn: {active_turn:?}\nPlanning work: {items:?}")
}

fn summarize_task(
    project: &Project,
    doc: &crate::artifacts::task_docs::TaskDocument,
    waiting_on_user: &mut Vec<String>,
    blocked_elsewhere: &mut Vec<String>,
) -> String {
    if let Some(dependency) = unresolved_dependency(project, doc) {
        blocked_elsewhere.push(format!(
            "{} waits for prerequisite {}",
            doc.title, dependency
        ));
    }
    let state = project.implementation_states.get(&doc.path);
    let status = state.map(|record| record.status);
    let pull_request_closed = state.is_some_and(|record| {
        record.pr_state == Some(crate::core::implementation::PullRequestState::Closed)
    });
    if pull_request_closed {
        waiting_on_user.push(format!("{}: Pull request closed", doc.title));
    }
    if let Some(failure) = project.queue.blocked.get(&doc.path) {
        let summary =
            crate::core::context_build::clip(&format!("{}: {}", doc.title, failure.message), 400);
        match failure.recovery {
            crate::core::implementation::RecoveryDisposition::UserAction
            | crate::core::implementation::RecoveryDisposition::ExplicitResume => {
                waiting_on_user.push(summary);
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
            | crate::core::implementation::ImplementationStatus::PullRequestClosed
            | crate::core::implementation::ImplementationStatus::Interrupted),
        ) => waiting_on_user.push(format!("{}: {}", doc.title, current.label())),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::planning_work::{Work, WorkStatus};

    #[test]
    fn manager_context_lists_planning_statuses_and_active_turn() {
        let mut active = Work::new(
            "planning:active".into(),
            "Current plan".into(),
            "Request".into(),
            "Planning in progress".into(),
        );
        active.status = WorkStatus::InProgress;
        let mut attention = Work::new(
            "planning:attention".into(),
            "Interrupted plan".into(),
            "Request".into(),
            "Continue in its conversation".into(),
        );
        attention.status = WorkStatus::NeedsAttention;

        let context = planning_context(&[attention, active], Some("planning:active"));

        assert!(context.contains("Active planning turn: Some(\"planning:active\")"));
        assert!(context.contains("Interrupted plan: NeedsAttention"));
        assert!(context.contains("Current plan: InProgress"));
    }
}
