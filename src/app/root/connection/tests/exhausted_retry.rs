use super::super::*;

struct UnusedHarness;

impl crate::harness::AiHarness for UnusedHarness {
    fn label(&self) -> String {
        "unused exhausted-retry fixture".into()
    }

    fn check_available(&self) -> Result<String, crate::error::AppError> {
        Ok(self.label())
    }

    fn execute(
        &self,
        _request: &crate::harness::PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, crate::error::AppError> {
        panic!("an exhausted reply must not be run again")
    }
}

#[test]
/// Production boundary: the app coordinator detects an exhausted persisted
/// retry before selecting a provider and records internal recovery state. The
/// fixture uses local planner files only; no Docker or network is needed.
fn exhausted_conflicts_stop_retrying_without_requesting_the_saved_answer_again() {
    let mut app = crate::app::root::board_tests::fixture();
    app.task_harness = Some(Box::new(UnusedHarness));
    let key = "planning:exhausted-retry";
    let slug;
    if let Screen::Connected(project) = &mut app.screen {
        let mut work = crate::core::planning_work::Work::new(
            key.into(),
            "Recover conflicted answer".into(),
            "Keep the approved branch selection.".into(),
            "An internal revision conflict occurred.".into(),
        );
        work.kind = crate::core::planning_work::WorkKind::Bug;
        project.planning_work.push(work);
        project.save_planning_work().unwrap();
        slug = project.chat_slug.clone();
        project.task_chats.ensure_loaded(&slug);
        let user = crate::domain::ChatMessage::new(
            crate::domain::ChatRole::User,
            "Keep release/2.1.",
            Some(key.into()),
        );
        let mut conflicts = Vec::new();
        for _ in 0..=crate::persistence::task_chats::MAX_PLANNING_CONFLICT_RETRIES {
            let mut conflict = crate::domain::ChatMessage::new(
                crate::domain::ChatRole::System,
                "Planning changed while the saved answer was being applied.",
                Some(key.into()),
            );
            crate::persistence::task_chats::TaskChats::mark_planning_conflict_retry(&mut conflict);
            conflicts.push(conflict);
        }
        project
            .task_chats
            .append(&slug, key, std::iter::once(user).chain(conflicts).collect())
            .unwrap();
    } else {
        panic!("fixture remains connected");
    }

    app.retry_pending_task_reply();

    let Screen::Connected(project) = &app.screen else {
        panic!("fixture remains connected");
    };
    assert!(!project.task_turns.contains_key(key));
    assert!(!project.task_chats.has_pending_user_reply(key));
    assert!(project.task_chats.has_failed_task_reply(key));
    let prompt = crate::app::manager::Manager::prompt_body(project, &[]);
    assert!(prompt.contains("Internal recovery required"));
    assert!(!prompt.contains("Waiting on user: [\"planning:exhausted-retry"));
    std::fs::remove_dir_all(slug).unwrap();
}

#[test]
/// Production boundary: after the final retry worker and its takeover both
/// expire, the app coordinator records durable recovery status and does not
/// select a provider. Temporary local files only; no Docker or network.
fn exhausted_retry_claim_takeovers_become_manager_recovery() {
    let mut app = crate::app::root::board_tests::fixture();
    app.task_harness = Some(Box::new(UnusedHarness));
    let key = "planning:exhausted-claims";
    let slug;
    if let Screen::Connected(project) = &mut app.screen {
        let mut work = crate::core::planning_work::Work::new(
            key.into(),
            "Recover claimed answer".into(),
            "Keep the existing branch selection.".into(),
            "Two internal retry workers exited before applying the answer.".into(),
        );
        work.kind = crate::core::planning_work::WorkKind::Bug;
        project.planning_work.push(work);
        project.save_planning_work().unwrap();
        slug = project.chat_slug.clone();
        project.task_chats.ensure_loaded(&slug);

        let now = chrono::Utc::now();
        let mut user = crate::domain::ChatMessage::new(
            crate::domain::ChatRole::User,
            "Keep release/2.1.",
            Some(key.into()),
        );
        user.ts = now - chrono::Duration::minutes(20);
        let user_id = user.id.clone();
        let mut conflict = crate::domain::ChatMessage::new(
            crate::domain::ChatRole::System,
            "The planning revision changed before the saved answer could be applied.",
            Some(key.into()),
        );
        conflict.ts = now - chrono::Duration::minutes(19);
        crate::persistence::task_chats::TaskChats::mark_planning_conflict_retry(&mut conflict);
        let mut claims = Vec::new();
        for minutes_ago in [17, 16] {
            let mut claim = crate::domain::ChatMessage::new(
                crate::domain::ChatRole::System,
                "Kool.ad/e is retrying this saved task reply internally.",
                Some(key.into()),
            );
            claim.id = format!(
                "internal-retry:planning-write-claim:{user_id}:1:{}",
                uuid::Uuid::new_v4()
            );
            claim.ts = now - chrono::Duration::minutes(minutes_ago);
            claims.push(claim);
        }
        project
            .task_chats
            .append(
                &slug,
                key,
                std::iter::once(user)
                    .chain([conflict])
                    .chain(claims)
                    .collect(),
            )
            .unwrap();
    } else {
        panic!("fixture remains connected");
    }

    app.retry_pending_task_reply();

    let Screen::Connected(project) = &app.screen else {
        panic!("fixture remains connected");
    };
    assert!(!project.task_turns.contains_key(key));
    assert!(!project.task_chats.has_pending_user_reply(key));
    assert!(project.task_chats.has_failed_task_reply(key));
    assert_eq!(
        project
            .planning_work
            .iter()
            .find(|work| work.key == key)
            .unwrap()
            .status,
        crate::core::planning_work::WorkStatus::NeedsAttention
    );
    let prompt = crate::app::manager::Manager::prompt_body(project, &[]);
    assert!(prompt.contains("Internal recovery required"));
    assert!(!prompt.contains("Waiting on user: [\"planning:exhausted-claims"));
    std::fs::remove_dir_all(slug).unwrap();
}
