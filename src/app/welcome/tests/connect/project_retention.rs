use super::*;

/// Production boundary: connecting another code repository must retain a
/// detached project's private TaskIntent history for a later reconnect. A
/// store test cannot prove the welcome path leaves unrelated project shells
/// untouched. Uses two temporary local Git repos and private stores only; no
/// provider, Docker, or network; low setup cost.
#[test]
fn connecting_another_project_retains_detached_task_history() {
    let _env = super::super::helpers::EnvSandbox::enter("project-shell-retention", false);
    let first_code = super::super::helpers::local_source_repo("retained-project");
    let other_code = super::super::helpers::local_source_repo("other-project");
    let mut first = attempt_connect(first_code.to_str().unwrap()).unwrap();
    let mut task = crate::core::planning_work::Work::new(
        String::new(),
        "Retain detached work".into(),
        "Preserve this task across a separate project connection.".into(),
        "The project may be reopened after another workspace is used.".into(),
    );
    task.kind = crate::core::planning_work::WorkKind::Feature;
    task.key = format!("task:{}", task.uid);
    task.task_intent_uid = Some(task.uid.clone());
    let expected_uid = task.uid.clone();
    first.planning_work.push(task);
    first.save_planning_work().unwrap();
    drop(first);

    let other = attempt_connect(other_code.to_str().unwrap()).unwrap();
    drop(other);

    let reopened = attempt_connect(first_code.to_str().unwrap()).unwrap();
    let recovered = crate::core::planning_work::load(&reopened.state.planning_store).unwrap();
    assert!(recovered.iter().any(|work| {
        work.uid == expected_uid && work.task_intent_uid.as_deref() == Some(expected_uid.as_str())
    }));
    assert!(
        reopened
            .state
            .planning_store
            .root
            .join("state/work")
            .is_dir()
    );
    drop(reopened);

    let _ = std::fs::remove_dir_all(first_code);
    let _ = std::fs::remove_dir_all(other_code);
    let _ = std::fs::remove_dir_all(&_env.home);
}
