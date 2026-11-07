use super::*;

#[test]
fn pages_cover_large_boards_prioritize_actionable_work_and_fence_mutations() {
    let (_temp, mut project, _feature) =
        crate::app::manager::tests::opportunities::initialized_repository();
    project.task_documents = (0..110)
        .map(|number| crate::artifacts::task_docs::TaskDocument {
            path: format!("tasks/{number:03}.md"),
            title: format!("Task {number:03}"),
            text: String::new(),
            identity: None,
            metadata: None,
            task_state: None,
            metadata_error: None,
        })
        .collect();
    for number in 90..100 {
        project.active_implementations.insert(
            format!("tasks/{number:03}.md"),
            crate::core::implementation::Controller::idle_fixture(),
        );
    }
    project.queue.blocked.insert(
        "tasks/109.md".into(),
        crate::core::implementation::Failure::new(
            crate::core::implementation::FailureKind::Other,
            crate::core::implementation::RecoveryDisposition::DoNotRetry,
            "blocked fixture",
        ),
    );
    let first = query(&project, None, 0, 50, &[]).unwrap();
    let second = query(&project, Some(&first.index_revision), 50, 60, &[]).unwrap();
    assert_eq!(first.total_entries, 110);
    assert_eq!(first.entries.len(), 50);
    assert_eq!(first.omitted_details, 60);
    assert_eq!(second.entries.len(), 60);
    assert_eq!(second.omitted_details, 0);
    assert_eq!(
        first.entries[0].status, "needs-attention",
        "attention work precedes active workers"
    );
    assert_eq!(first.entries[1].status, "running");
    assert_eq!(first.entries[1].kind, super::entries::TaskIndexKind::Task);
    assert_eq!(
        crate::app::manager::prompt::tasks::active_worker_progress(&project).len(),
        10
    );
    let focused = query(&project, None, 0, 12, &["task:tasks/050.md".into()]).unwrap();
    assert_eq!(focused.entries[11].id, "task:tasks/050.md");
    assert!(
        query(
            &project,
            Some(&focused.index_revision),
            12,
            12,
            &["task:tasks/050.md".into()]
        )
        .is_ok()
    );
    assert!(matches!(
        query(&project, Some(&focused.index_revision), 12, 12, &[]),
        Err(TaskIndexError::StaleRevision { .. })
    ));
    let mut all = first.entries;
    all.extend(second.entries);
    assert_eq!(all.len(), 110);
    assert_eq!(
        all.iter()
            .map(|entry| &entry.id)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        110
    );

    let stable_first = query(&project, Some(&second.index_revision), 0, 50, &[]).unwrap();
    project.task_documents.reverse();
    let shuffled_first = query(&project, Some(&second.index_revision), 0, 50, &[]).unwrap();
    assert_eq!(
        stable_first
            .entries
            .iter()
            .map(|entry| &entry.id)
            .collect::<Vec<_>>(),
        shuffled_first
            .entries
            .iter()
            .map(|entry| &entry.id)
            .collect::<Vec<_>>()
    );
    project.task_documents[0].title.push_str(" changed");
    assert!(matches!(
        query(&project, Some(&second.index_revision), 0, 50, &[]),
        Err(TaskIndexError::StaleRevision { .. })
    ));
}

#[test]
fn prompt_index_reports_actual_omissions_from_its_byte_budget() {
    let (_temp, mut project, _feature) =
        crate::app::manager::tests::opportunities::initialized_repository();
    project.task_documents = (0..160)
        .map(|number| crate::artifacts::task_docs::TaskDocument {
            path: format!("tasks/{number:03}.md"),
            title: "A deliberately long task title that consumes prompt space".into(),
            text: String::new(),
            identity: None,
            metadata: None,
            task_state: None,
            metadata_error: None,
        })
        .collect();
    let (metrics, rows) = render(&project, &[]);
    assert!(metrics.contains("omitted "));
    assert!(metrics.contains("query next page"));
    assert!(rows.len() <= 5 * 1024);
    let included = rows.lines().count();
    assert!(metrics.contains(&format!("included {included} entries")));
    assert!(included < 160);
    assert!(metrics.contains(&format!("next page at offset Some({included})")));
}

#[test]
fn index_page_and_render_cost_stay_bounded_as_board_grows() {
    let (_temp, mut project, _feature) =
        crate::app::manager::tests::opportunities::initialized_repository();
    project.task_documents = (0..1_000)
        .map(|number| crate::artifacts::task_docs::TaskDocument {
            path: format!("tasks/{number:04}.md"),
            title: format!("Task {number:04}"),
            text: String::new(),
            identity: None,
            metadata: None,
            task_state: None,
            metadata_error: None,
        })
        .collect();
    let page = query(&project, None, 0, 256, &[]).unwrap();
    let (metrics, rows) = render(&project, &[]);
    let legacy_recent_eight_bytes = project
        .task_documents
        .iter()
        .rev()
        .take(8)
        .map(|document| format!("{}: {} — To do", document.path, document.title))
        .collect::<Vec<_>>()
        .join("\n")
        .len();
    assert_eq!(page.total_entries, 1_000);
    assert_eq!(page.entries.len(), 256);
    assert_eq!(page.omitted_details, 744);
    // The former Manager prompt emitted only eight task summaries regardless
    // of board size; the page API covers more while retaining a fixed bound.
    assert!(page.entries.len() > 8);
    assert!(rows.len() > legacy_recent_eight_bytes);
    assert!(rows.len() <= 5 * 1024);
    let included = rows.lines().count();
    assert!(metrics.contains(&format!("included {included} entries")));
    assert!(metrics.contains(&format!(
        "omitted {} by prompt byte budget",
        1_000 - included
    )));
}
