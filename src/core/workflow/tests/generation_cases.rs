use super::*;

#[test]
fn rejects_incomplete_uncovered_or_misordered_tasks_without_writes() {
    let mut s = state("invalid");
    mark_ready(&mut s);
    let mut task = story();
    task.implementation_steps.clear();
    assert!(generation(&s, vec![task]).is_err());
    let mut task = story();
    task.dependencies = vec![1];
    assert!(generation(&s, vec![task]).is_err());
    let mut task = story();
    task.success_criteria.clear();
    assert!(generation(&s, vec![task]).is_err());
    let mut task = story();
    task.scope_items = vec![2];
    assert!(generation(&s, vec![task]).is_err());
    assert!(!s.repo_root.join(".koolade-packet/planning/tasks").exists());
    std::fs::remove_dir_all(s.repo_root).unwrap();
}

#[test]
fn stale_spec_cannot_write_tasks_and_revisions_preserve_previous_batches() {
    let mut s = state("revision");
    mark_ready(&mut s);
    let nt = generation(&s, vec![story()]).unwrap();
    let spec_path = s.repo_root.join(crate::artifacts::SPEC_FILE);
    let original_spec = std::fs::read(&spec_path).unwrap();
    std::fs::write(&spec_path, "# External change").unwrap();
    assert!(apply::apply(&mut s, &nt).is_err());
    assert!(!s.repo_root.join(".koolade-packet/planning/tasks").exists());
    assert!(
        !PlannerState::load(&s.repo_root)
            .unwrap()
            .workflow
            .ready(Some("# External change"))
    );
    std::fs::write(&spec_path, original_spec).unwrap();
    apply::apply(&mut s, &nt).unwrap();
    let first = s
        .repo_root
        .join(".koolade-packet/planning/tasks/saved-searches/001-persist-named-search-filters.md");
    let original = std::fs::read(&first).unwrap();
    mark_ready(&mut s);
    let mut task = story();
    task.title = "Persist the revised search record".into();
    let nt = generation(&s, vec![task]).unwrap();
    apply::apply(&mut s, &nt).unwrap();
    assert!(
        s.repo_root
            .join(
                ".koolade-packet/planning/tasks/saved-searches-02/001-persist-the-revised-search-record.md"
            )
            .exists()
    );
    assert_eq!(std::fs::read(first).unwrap(), original);
    std::fs::remove_dir_all(s.repo_root).unwrap();
}

#[test]
fn batch_publication_rolls_back_on_metadata_failure() {
    let mut s = state("rollback");
    mark_ready(&mut s);
    let nt = generation(&s, vec![story()]).unwrap();
    // Exercise the writer's rollback after publication without mutating a real project.
    std::fs::remove_file(s.repo_root.join(WORKFLOW_FILE)).unwrap();
    std::fs::create_dir(s.repo_root.join(WORKFLOW_FILE)).unwrap();
    let mut workflow = nt.workflow.unwrap();
    assert!(
        crate::artifacts::task_docs::write_batch(
            &s.repo_root,
            &nt.task_batch.unwrap(),
            &mut workflow
        )
        .is_err()
    );
    assert_eq!(
        std::fs::read_dir(s.repo_root.join(".koolade-packet/planning/tasks"))
            .unwrap()
            .count(),
        0
    );
    assert!(workflow.task_batches.is_empty());
    std::fs::remove_dir_all(s.repo_root).unwrap();
}

#[cfg(unix)]
#[test]
fn generated_paths_cannot_escape_through_symlinks() {
    let mut s = state("symlink");
    mark_ready(&mut s);
    let nt = generation(&s, vec![story()]).unwrap();
    std::fs::create_dir_all(s.repo_root.join(".koolade-packet/planning")).unwrap();
    std::os::unix::fs::symlink(
        std::env::temp_dir(),
        s.repo_root.join(".koolade-packet/planning/tasks"),
    )
    .unwrap();
    assert!(apply::apply(&mut s, &nt).is_err());
    assert_eq!(
        crate::artifacts::task_docs::slug("../../Bad / Feature"),
        "bad-feature"
    );
    assert!(crate::artifacts::task_docs::slug(&"界".repeat(100)).len() <= 56);
    std::fs::remove_dir_all(s.repo_root).unwrap();
}
#[test]
fn blocking_questions_prevent_a_readiness_offer() {
    let mut s = state("blocking");
    s.items.push(crate::domain::OpenItem::new(
        "CLR-001".into(),
        crate::domain::Priority::Blocking,
        crate::domain::ItemKind::Question,
        "General".into(),
        Some("All".into()),
        "Which user workflow is required?".into(),
        "Scope cannot be established yet.".into(),
    ));
    let mut env = envelope();
    env.interview = Some(brief());
    assert!(
        validation::validate(&env, &s, &s.effective_user())
            .unwrap_err()
            .iter()
            .any(|e| e.contains("blocking"))
    );
    std::fs::remove_dir_all(s.repo_root).unwrap();
}
