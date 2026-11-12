use super::super::super::*;
use crate::app::root::board_tests::support::restart_fixture;
use crate::core::planning_work::{self, Work, WorkKind, WorkStatus};
use crate::persistence::execution_records;
use std::process::Command;

#[path = "restart_recovery/persistence.rs"]
mod persistence;
#[path = "restart_recovery/plan_snapshot.rs"]
mod plan_snapshot;
#[path = "restart_recovery/resume.rs"]
mod resume;
use persistence::reload_task_documents;

fn git(root: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(output.status.success(), "git command failed");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

/// Production boundary: app restart reloads the real PlanningStore and queue,
/// while operator-local approvals and execution locks live outside the code
/// clone. This checks durable state and worker admission across that boundary;
/// a unit test of a serializer cannot prove the app reloads the same roots.
/// It uses local Git and temporary files only. No Docker or network is needed;
/// setup is moderate because it creates two clones and a connected app fixture.
#[test]
fn restart_recovers_partial_task_work_without_duplicate_child_or_false_completion() {
    let _environment = crate::test_support::lock_process_environment();
    let home = std::env::temp_dir().join(format!("koolade-restart-{}", uuid::Uuid::new_v4()));
    let _home = crate::test_support::KooladeHomeGuard::set(&home);
    let _shield = crate::core::gitops::test_support::shield("restart-task-recovery");
    let mut app = crate::app::root::board_tests::support::fixture();
    let (repo, planning_store) = match &app.screen {
        Screen::Connected(project) => (
            project.state.repo_root.clone(),
            project.state.planning_store.clone(),
        ),
        _ => panic!("fixture must be connected"),
    };

    let task_docs = match &app.screen {
        Screen::Connected(project) => project.task_documents.clone(),
        _ => panic!("fixture must be connected"),
    };
    let task_paths = task_docs
        .iter()
        .map(|doc| doc.path.clone())
        .collect::<Vec<_>>();
    let mut stale_plan = plan_snapshot::seed_feature_file(&repo);
    persistence::persist_fixture_identities(&planning_store, &task_docs);
    let mut tasks = [
        Work::new(
            "ROOT-1".into(),
            "Implement first half".into(),
            "Preserve first child progress".into(),
            String::new(),
        ),
        Work::new(
            "ROOT-2".into(),
            "Implement second half".into(),
            "Preserve second child progress".into(),
            String::new(),
        ),
        Work::new(
            "ROOT-3".into(),
            "Integrate the children".into(),
            "Keep integration pending".into(),
            String::new(),
        ),
    ];
    for task in &mut tasks {
        task.kind = WorkKind::Feature;
        task.status = WorkStatus::InProgress;
        task.feature_id = Some("CHG-468".into());
    }
    for (task, doc) in tasks.iter_mut().zip(task_docs.iter()) {
        task.uid = doc.identity.as_ref().unwrap().uid.clone();
        task.key = format!("task:{}", task.uid);
    }

    let clones =
        persistence::prepare_partial_clones(&planning_store, &repo, &task_docs, &mut tasks);
    if let Screen::Connected(project) = &mut app.screen {
        // The fixture replaces the local origin with a mapped-code bare remote.
        // Explicit refresh repins the same published commit under that origin identity.
        project.state.resync().unwrap();
    }
    planning_work::save(&planning_store, &tasks).unwrap();
    if let Screen::Connected(project) = &mut app.screen {
        project.planning_work = planning_work::load(&planning_store).unwrap();
        plan_snapshot::persist_card(project, &mut stale_plan);
    }
    crate::persistence::feature_approvals::record(
        &planning_store.root,
        "CHG-468",
        "approved-contract-sha256",
    )
    .unwrap();

    let active_task_id = tasks[0].uid.clone();
    let task_lock = execution_records::try_task_lock(&active_task_id)
        .unwrap()
        .expect("first worker acquires the active child");
    let Screen::Connected(project) = &mut app.screen else {
        panic!("fixture must be connected")
    };
    // A pending saved answer and a cancelled sibling must survive the same
    // restart as the active implementation lease. Neither may be recreated
    // as fresh work when the board reloads.
    let answer_key = "planning:restart-pending-answer";
    let mut cancelled_ids = project.cancelled_work.clone();
    cancelled_ids.insert(crate::persistence::cancelled_work::task_id(&task_docs[1]));
    project.state.baseline_planning_revision = crate::persistence::cancelled_work::save_expected(
        &project.state.planning_store,
        &cancelled_ids,
        &project.state.baseline_planning_revision,
    )
    .unwrap();
    project.cancelled_work = cancelled_ids;
    let chat_slug = project.chat_slug.clone();
    project.task_chats.ensure_loaded(&chat_slug);
    let answer = crate::domain::ChatMessage::new(
        crate::domain::ChatRole::User,
        "Keep the release branch as the destination.",
        Some(answer_key.into()),
    );
    let mut retry = crate::domain::ChatMessage::new(
        crate::domain::ChatRole::System,
        "The saved answer is retained for internal retry after restart.",
        None,
    );
    crate::persistence::task_chats::TaskChats::mark_planning_conflict_retry(&mut retry);
    project
        .task_chats
        .append(&chat_slug, answer_key, vec![answer, retry])
        .unwrap();

    // Advance the mapped repository baseline after task clones were created.
    // Recovery must retain each task's approved source SHA and must not claim
    // that the stale task baseline was silently refreshed or completed.
    std::fs::write(
        repo.join("baseline-advanced.txt"),
        "new destination baseline\n",
    )
    .unwrap();
    std::fs::write(&stale_plan.path, &stale_plan.updated_body).unwrap();
    git(
        &repo,
        &[
            "add",
            "baseline-advanced.txt",
            stale_plan.path.to_str().unwrap(),
        ],
    );
    git(
        &repo,
        &[
            "-c",
            "user.name=Recovery Test",
            "-c",
            "user.email=recovery@example.test",
            "commit",
            "-m",
            "advance remote baseline",
        ],
    );
    git(&repo, &["push", "origin", "main"]);
    let persisted_states = persistence::save_partial_states(
        &planning_store,
        &task_docs,
        &tasks,
        &clones,
        &mut project.implementation_states,
    );
    let mut queue = crate::core::implementation_queue::Queue::default();
    queue.running = true;
    queue.bind_task_documents(&task_docs).unwrap();
    let first_task = task_docs[0].path.clone();
    let second_task = task_docs[1].path.clone();
    queue.current_ticket = Some(first_task.clone());
    queue.in_flight.insert(first_task.clone());
    queue.save(&repo).unwrap();

    let restarted = restart_fixture(app);
    app = restarted;
    let Screen::Connected(project) = &mut app.screen else {
        panic!("restarted fixture must remain connected")
    };
    project
        .state
        .resync()
        .expect("startup refreshes the changed code baseline and feature snapshot");
    // The generic restart fixture rebuilds in-memory fields from defaults;
    // production connection startup reloads this durable cancellation set.
    project.cancelled_work =
        crate::persistence::cancelled_work::load(&project.state.planning_store).unwrap();
    project.task_documents = reload_task_documents(&project.state.planning_store, &task_paths);
    assert_eq!(project.task_documents.len(), task_docs.len());
    for ((doc, work), before_restart) in project.task_documents.iter().zip(&tasks).zip(&task_docs) {
        assert_eq!(
            doc.identity.as_ref().map(|identity| &identity.uid),
            Some(&work.uid)
        );
        assert_eq!(doc.identity, before_restart.identity);
    }
    project.queue = crate::core::implementation_queue::Queue::load(&repo).unwrap();
    project
        .queue
        .bind_task_documents(&project.task_documents)
        .unwrap();
    project.implementation_states = crate::core::implementation::load_board_states_with_store(
        &repo,
        &project.state.planning_store,
    );

    project.planning_work = planning_work::load(&project.state.planning_store).unwrap();
    plan_snapshot::refresh_after_restart(project, &stale_plan);

    let restored = project.planning_work.clone();
    let restored_tasks = restored
        .iter()
        .filter(|work| tasks.iter().any(|task| task.uid == work.uid))
        .collect::<Vec<_>>();
    assert_eq!(restored_tasks.len(), 3);
    assert!(
        restored_tasks
            .iter()
            .all(|work| work.status == WorkStatus::InProgress)
    );
    for task in &tasks {
        let work = restored_tasks
            .iter()
            .find(|work| work.uid == task.uid)
            .unwrap();
        let intent = work
            .git_intent
            .as_ref()
            .expect("persisted approved Git intent");
        intent.validate().unwrap();
        let index = tasks.iter().position(|task| task.uid == work.uid).unwrap();
        assert_eq!(intent.source_sha, clones.shas[index]);
        assert_eq!(intent.source_ref, persisted_states[index].branch);
    }
    assert!(restored.iter().any(|work| {
        work.kind == WorkKind::PlanConfirmation
            && work.plan_confirmation.as_ref().is_some_and(|payload| {
                payload.snapshot_content_hash != stale_plan.old_hash
                    && payload.snapshot_spec_revision != stale_plan.old_spec_revision
            })
    }));
    assert!(crate::persistence::feature_approvals::contains(
        &project.state.planning_store.root,
        "CHG-468",
        "approved-contract-sha256"
    ));
    assert_eq!(
        project.queue.current_ticket.as_deref(),
        Some(first_task.as_str())
    );
    assert!(project.queue.in_flight.contains(&first_task));
    assert!(!project.queue.in_flight.contains(&second_task));
    let cancelled_id = crate::persistence::cancelled_work::task_id(&task_docs[1]);
    assert!(project.cancelled_work.contains(&cancelled_id));
    project.task_chats.ensure_loaded(&project.chat_slug);
    assert_eq!(
        project
            .task_chats
            .pending_planning_conflict_retries("planning:restart-pending-answer"),
        Some(1)
    );

    assert!(
        execution_records::try_task_lock(&active_task_id)
            .unwrap()
            .is_none()
    );
    for (path, expected_sha) in clones.paths.iter().zip(&clones.shas) {
        assert_eq!(git(path, &["rev-parse", "HEAD"]), *expected_sha);
        assert!(git(path, &["status", "--porcelain"]).contains("partial-progress.txt"));
        assert!(std::fs::read_to_string(path.join("partial-progress.txt")).is_ok());
    }
    assert_ne!(git(&repo, &["rev-parse", "HEAD"]), clones.shas[0]);
    let task_uids = tasks
        .iter()
        .map(|task| task.uid.clone())
        .collect::<Vec<_>>();
    persistence::assert_implementation_recovery(
        &project.implementation_states,
        &persisted_states,
        &task_uids,
    );
    resume::exercise_task_details_resume(
        &mut app,
        &first_task,
        &active_task_id,
        &clones.paths[0],
        &clones.shas[0],
        task_lock,
    );
}
