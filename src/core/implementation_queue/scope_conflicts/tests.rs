use super::*;
use crate::core::implementation::Implementation;
use crate::core::implementation_queue::next_ready_ticket_with_running_scopes;
use std::collections::BTreeMap;

fn task(number: usize, repository: &str, affected: Option<&str>) -> TaskDocument {
    let scope = affected
        .map(|path| format!("\n## Affected files and components\n- `{path}`\n"))
        .unwrap_or_default();
    TaskDocument {
        path: format!("tasks/{number:03}-task.md"),
        title: format!("Task {number}"),
        text: format!("# Task {number}\n\nRepository: {repository}{scope}"),
        identity: None,
        metadata: None,
        metadata_error: None,
    }
}

#[test]
fn automatic_queue_skips_a_task_that_overlaps_running_scope() {
    let docs = vec![
        task(1, "root", Some("src/services/StartupService.cs")),
        task(2, "root", Some("Services/Xor/StartupService.cs")),
        task(3, "root", Some("src/data/StartupRepository.cs")),
    ];
    let running = BTreeSet::from([docs[0].path.clone()]);
    assert_eq!(
        next_ready_ticket_with_running_scopes(
            &docs,
            &BTreeMap::<String, Implementation>::new(),
            &running,
            &running,
        )
        .unwrap(),
        Some(docs[2].path.clone())
    );
}

#[test]
fn manual_start_reports_overlapping_active_scope() {
    let docs = vec![
        task(1, "root", Some("src/services/StartupService.cs")),
        task(2, "root", Some("Services/Xor/StartupService.cs")),
    ];
    let running = BTreeSet::from([docs[0].path.clone()]);
    let reason = active_scope_conflict(&docs, &docs[1].path, &running).unwrap();
    assert!(reason.contains("Planning-scope heuristic"));
    assert!(reason.contains("Actual Git changes are checked again before integration"));
    assert!(reason.contains("Task 1") && reason.contains("startupservice"));
}

#[test]
fn unknown_scope_serializes_with_running_task() {
    let docs = vec![
        task(1, "root", Some("src/services/StartupService.cs")),
        task(2, "root", None),
    ];
    let running = BTreeSet::from([docs[0].path.clone()]);
    assert!(
        active_scope_conflict(&docs, &docs[1].path, &running)
            .is_some_and(|reason| reason.contains("unspecified"))
    );
}

#[test]
fn directory_scope_overlaps_a_nested_file() {
    let docs = vec![
        task(1, "root", Some("src/services")),
        task(2, "root", Some("src/services/StartupService.cs")),
    ];
    let running = BTreeSet::from([docs[0].path.clone()]);
    assert!(active_scope_conflict(&docs, &docs[1].path, &running).is_some());
}

#[test]
fn nested_directory_scopes_overlap_in_either_order() {
    let docs = vec![
        task(1, "root", Some("src/services")),
        task(2, "root", Some("src/services/controllers")),
    ];
    let parent_running = BTreeSet::from([docs[0].path.clone()]);
    let child_running = BTreeSet::from([docs[1].path.clone()]);
    assert!(active_scope_conflict(&docs, &docs[1].path, &parent_running).is_some());
    assert!(active_scope_conflict(&docs, &docs[0].path, &child_running).is_some());
}

#[test]
fn same_path_in_different_repositories_does_not_conflict() {
    let docs = vec![
        task(1, "server", Some("src/services/StartupService.cs")),
        task(2, "client", Some("src/services/StartupService.cs")),
    ];
    let running = BTreeSet::from([docs[0].path.clone()]);
    assert_eq!(active_scope_conflict(&docs, &docs[1].path, &running), None);
}
