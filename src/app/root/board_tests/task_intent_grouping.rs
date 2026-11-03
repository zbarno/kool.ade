use super::*;

/// Production rendering boundary: one TaskIntent header is painted in every
/// lane containing owned children. The grouping helper alone cannot prove that
/// the actual lane painter renders both views. Uses the synthetic fixture, no
/// external services, low setup cost.
#[test]
fn same_task_intent_header_is_rendered_in_two_child_lanes() {
    let mut app = fixture();
    let Screen::Connected(project) = &mut app.screen else {
        unreachable!()
    };
    let mut root = crate::core::planning_work::Work::new(
        String::new(),
        "One request across columns".into(),
        "Preserve the root while children move through the board".into(),
        "Planning".into(),
    );
    root.kind = crate::core::planning_work::WorkKind::Feature;
    root.status = crate::core::planning_work::WorkStatus::InProgress;
    root.key = format!("task:{}", root.uid);
    root.task_intent_uid = Some(root.uid.clone());
    let root_uid = root.uid.clone();
    let mut ready = crate::core::planning_work::Work::new(
        String::new(),
        "Ready child".into(),
        "A child in the first lane".into(),
        String::new(),
    );
    ready.kind = crate::core::planning_work::WorkKind::Question;
    ready.status = crate::core::planning_work::WorkStatus::Todo;
    ready.parent_uid = Some(root_uid.clone());
    ready.task_intent_uid = Some(root_uid.clone());
    let mut active = crate::core::planning_work::Work::new(
        String::new(),
        "Active child".into(),
        "A child in the second lane".into(),
        String::new(),
    );
    active.kind = crate::core::planning_work::WorkKind::Bug;
    active.status = crate::core::planning_work::WorkStatus::InProgress;
    active.parent_uid = Some(root_uid.clone());
    active.task_intent_uid = Some(root_uid);
    project.planning_work = vec![root, ready, active];
    project.save_planning_work().unwrap();

    let ctx = super::mockup_layout::styled_context();
    for _ in 0..4 {
        frame(&mut app, &ctx, vec![]);
    }
    let rendered = canvas_text(&frame(&mut app, &ctx, vec![]));
    assert_eq!(rendered.matches("One request across columns").count(), 2);
}
