use super::naming::{batch_slug, is_task_story_filename, render, task_name};
use crate::core::workflow::{TaskBatch, TaskStory};

#[test]
fn feature_tasks_carry_the_feature_id_in_file_heading_and_dependencies() {
    let first = TaskStory {
        title: "Generate ID".into(),
        ..Default::default()
    };
    let second = TaskStory {
        title: "Use generated ID".into(),
        dependencies: vec![1],
        ..Default::default()
    };
    let batch = TaskBatch {
        brief: crate::core::workflow::InterviewBrief {
            feature_name: "Add ID to features (F10)".into(),
            ..Default::default()
        },
        specification: String::new(),
        feature_id: Some("F10".into()),
        contract: None,
        stories: vec![first.clone(), second.clone()],
    };
    let names = batch
        .stories
        .iter()
        .enumerate()
        .map(|(index, story)| task_name(batch.feature_id.as_deref(), index, story))
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        ["F10-TASK-generate-id.md", "F10-TASK-use-generated-id.md"]
    );
    assert_eq!(batch_slug(&batch), "F10-add-id-to-features");
    let rendered = render(&batch, 1, &second, &names);
    assert!(rendered.starts_with("# F10-TASK-use-generated-id — Use generated ID"));
    assert!(rendered.contains("[F10-TASK-generate-id](F10-TASK-generate-id.md)"));
    assert!(names.iter().all(|name| is_task_story_filename(name)));
    assert!(is_task_story_filename("001-legacy-task.md"));
}
