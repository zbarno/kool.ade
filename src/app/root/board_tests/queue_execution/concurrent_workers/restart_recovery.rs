use super::super::super::*;
use crate::app::root::board_tests::support::restart_fixture;
use crate::core::planning_work::{self, Work, WorkKind, WorkStatus};
use crate::persistence::execution_records;
use std::process::Command;

#[path = "restart_recovery/persistence.rs"]
mod persistence;
use persistence::reload_task_documents;

fn git(root: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
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

    let clone_root = repo.parent().unwrap().join("partial-task-clones");
    std::fs::create_dir_all(&clone_root).unwrap();
    let origin = repo.to_string_lossy().into_owned();
    let mut clone_paths = Vec::new();
    let mut clone_shas = Vec::new();
    for (index, (name, progress)) in [
        ("child-one", "first child partial output"),
        ("child-two", "second child partial output"),
        ("integration", "pending integration output"),
    ]
    .into_iter()
    .enumerate()
    {
        let path = clone_root.join(name);
        let output = Command::new("git")
            .args(["clone", "-q", &origin, path.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "clone failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        git(&path, &["config", "user.name", "Recovery Test"]);
        git(&path, &["config", "user.email", "recovery@example.test"]);
        let branch = format!("koolade/{}", tasks[index].uid);
        git(&path, &["checkout", "-q", "-b", &branch]);
        std::fs::write(path.join("partial-progress.txt"), progress).unwrap();
        let sha = git(&path, &["rev-parse", "HEAD"]);
        let intent = crate::core::git_intent::GitIntentV1::approve(
            "root".into(),
            branch,
            sha.clone(),
            "main".into(),
            crate::core::git_intent::PublicationModeV1::HoldForReview,
            None,
        )
        .unwrap();
        intent.validate().unwrap();
        tasks[index].source_branch = Some(intent.source_ref.clone());
        tasks[index].destination_branch = Some(intent.destination_ref.clone());
        tasks[index].git_intent = Some(intent.clone());
        tasks[index].git_intents.insert("root".into(), intent);
        clone_shas.push(sha);
        assert!(git(&path, &["status", "--porcelain"]).contains("partial-progress.txt"));
        clone_paths.push(path);
    }
    planning_work::save(&planning_store, &tasks).unwrap();
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
    let template = project
        .implementation_states
        .values()
        .next()
        .expect("fixture implementation template")
        .clone();
    let mut persisted_states = Vec::new();
    for index in 0..3 {
        let mut state = template.clone();
        state.ticket = task_docs[index].path.clone();
        state.task_uid = Some(tasks[index].uid.clone());
        state.branch = format!("koolade/{}", tasks[index].uid);
        state.source_branch = Some(state.branch.clone());
        state.source_commit = Some(clone_shas[index].clone());
        state.base_commit = clone_shas[index].clone();
        state.task_repository = clone_paths[index].clone();
        state.task_repository_kind = crate::core::implementation::TaskRepositoryKind::Clone;
        state.task_repository_ready = true;
        state.task_repositories = vec![clone_paths[index].clone()];
        state.task_repository_commits =
            std::collections::BTreeMap::from([("root".into(), clone_shas[index].clone())]);
        state.status = if index == 2 {
            crate::core::implementation::ImplementationStatus::WaitingToMerge
        } else {
            crate::core::implementation::ImplementationStatus::Implementing
        };
        let dir = crate::core::implementation::state_dir_for_task(
            &planning_store.root,
            &state.ticket,
            state.task_uid.as_deref(),
        )
        .unwrap();
        crate::core::implementation::save(&dir, &state).unwrap();
        project
            .implementation_states
            .insert(state.ticket.clone(), state.clone());
        persisted_states.push(state);
    }
    let mut queue = crate::core::implementation_queue::Queue::default();
    queue.running = true;
    queue.bind_task_documents(&task_docs).unwrap();
    let first_task = task_docs[0].path.clone();
    let second_task = task_docs[1].path.clone();
    queue.current_ticket = Some(first_task.clone());
    queue.in_flight.insert(first_task.clone());
    queue.in_flight.insert(second_task.clone());
    queue.save(&repo).unwrap();

    let restarted = restart_fixture(app);
    app = restarted;
    let Screen::Connected(project) = &mut app.screen else {
        panic!("restarted fixture must remain connected")
    };
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

    let restored = planning_work::load(&project.state.planning_store).unwrap();
    assert_eq!(restored.len(), 3);
    assert!(
        restored
            .iter()
            .all(|work| work.status == WorkStatus::InProgress)
    );
    assert_eq!(
        restored[0].task_intent_root_uid(),
        Some(restored[0].uid.as_str())
    );
    assert_eq!(
        restored[1].task_intent_root_uid(),
        Some(restored[1].uid.as_str())
    );
    assert_eq!(
        restored[2].status,
        WorkStatus::InProgress,
        "integration stays pending"
    );
    for work in &restored {
        let intent = work
            .git_intent
            .as_ref()
            .expect("persisted approved Git intent");
        intent.validate().unwrap();
        let index = tasks.iter().position(|task| task.uid == work.uid).unwrap();
        assert_eq!(intent.source_sha, clone_shas[index]);
        assert_eq!(intent.source_ref, persisted_states[index].branch);
    }
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
    assert!(project.queue.in_flight.contains(&second_task));

    assert!(
        execution_records::try_task_lock(&active_task_id)
            .unwrap()
            .is_none()
    );
    for (path, expected_sha) in clone_paths.iter().zip(clone_shas) {
        assert_eq!(git(path, &["rev-parse", "HEAD"]), expected_sha);
        assert!(git(path, &["status", "--porcelain"]).contains("partial-progress.txt"));
        assert!(std::fs::read_to_string(path.join("partial-progress.txt")).is_ok());
    }
    let task_uids = tasks
        .iter()
        .map(|task| task.uid.clone())
        .collect::<Vec<_>>();
    persistence::assert_implementation_recovery(
        &project.implementation_states,
        &persisted_states,
        &task_uids,
    );
    drop(task_lock);
}
