use super::super::session::Project;
mod tasks;
use tasks::{active_worker_progress, planning_context, summarize_task};

pub(super) fn build(project: &Project, events: &[String]) -> String {
    let user = project.state.effective_user();
    let mut blocked_elsewhere = project
        .state
        .items
        .iter()
        .filter(|item| {
            item.status == crate::domain::ItemStatus::Open && !item.blocked_by.is_empty()
        })
        .map(|item| format!("{} waits for {}", item.id, item.blocked_by.join(", ")))
        .collect::<Vec<_>>();
    let eligible = crate::core::routing::human_actionable_items(
        &project.state.items,
        &user,
        &project.state.config.stakeholders,
    );
    let mut waiting_on_user = Vec::new();
    let mut saved_replies_pending = Vec::new();
    let mut internal_recovery = Vec::new();
    for item in eligible {
        if project
            .task_chats
            .has_ambiguous_identity(item.conversation_key())
        {
            blocked_elsewhere.push(format!(
                "{} has conflicting board conversation identities that need internal repair",
                item.id
            ));
            continue;
        }
        if project
            .task_chats
            .has_failed_task_reply(item.conversation_key())
        {
            internal_recovery.push(format!(
                "{}: the submitted answer failed during internal processing; recover from its saved conversation without asking the user to repeat it",
                item.id
            ));
            continue;
        }
        let summary = crate::core::context_build::clip(
            &format!("{}: {} ({})", item.id, item.question, item.reason),
            400,
        );
        if tasks::conversation_reply_pending(project, item.conversation_key()) {
            saved_replies_pending.push(format!(
                "{}: user reply is saved and needs internal processing",
                item.id
            ));
        } else {
            waiting_on_user.push(summary);
        }
    }
    let mut tasks = project
        .task_documents
        .iter()
        .filter(|doc| !doc.path.ends_with("/README.md"))
        .map(|doc| {
            summarize_task(
                project,
                doc,
                &mut waiting_on_user,
                &mut saved_replies_pending,
                &mut internal_recovery,
                &mut blocked_elsewhere,
            )
        })
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
    let internal_recovery = internal_recovery.into_iter().take(12).collect::<Vec<_>>();
    let active_progress = active_worker_progress(project);
    let update = crate::core::context_build::clip(
        &format!(
            "PROJECT MANAGER UPDATE\nProject: {}\nEvents: {:?}\nTask count: {}\nBoard tasks: {:?}\n{}\nActive worker progress: {:?}\nActive worker: {:?}\nQueue running: {}\nWaiting on user: {:?}\nSaved user replies awaiting internal processing: {:?}\nInternal recovery required: {:?}\nBlocked on external work or dependencies: {:?}",
            project.state.title,
            events.iter().rev().take(8).collect::<Vec<_>>(),
            project.task_documents.len(),
            tasks,
            planning_work,
            active_progress,
            project.active_implementations.keys().collect::<Vec<_>>(),
            project.queue.running,
            waiting_on_user,
            saved_replies_pending,
            internal_recovery,
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

#[cfg(test)]
mod tests {
    use super::tasks::progress_label;
    use super::*;
    use crate::core::planning_work::{Work, WorkStatus};

    #[test]
    fn manager_receives_a_sanitized_worker_phase() {
        assert_eq!(
            progress_label(Some("koolade_bash · {\"command\":\"printf secret\"}")),
            "running a sandbox command"
        );
        assert_eq!(
            progress_label(Some("Verifying: dotnet build")),
            "running required verification"
        );
        assert_eq!(
            progress_label(Some(
                "Refreshing public NuGet vulnerability data, then retrying verification…"
            )),
            "refreshing the public NuGet audit feed before retrying verification"
        );
        assert_eq!(progress_label(None), "active; no current progress signal");
    }

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
