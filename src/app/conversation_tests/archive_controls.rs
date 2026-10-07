use super::*;

#[test]
fn all_resolved_planning_kinds_have_persistent_archive_controls() {
    for kind in [
        crate::domain::ItemKind::Question,
        crate::domain::ItemKind::Assumption,
        crate::domain::ItemKind::Ambiguity,
        crate::domain::ItemKind::Ownership,
    ] {
        let mut app = fixture();
        let Screen::Connected(p) = &mut app.screen else {
            panic!()
        };
        p.task_documents.clear();
        let mut item = OpenItem::new(
            "CLR-001".into(),
            crate::domain::Priority::Normal,
            kind,
            "General".into(),
            None,
            "Resolved planning work".into(),
            "Answered".into(),
        );
        item.status = crate::domain::ItemStatus::Resolved;
        p.state.resolved_items = vec![item];
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, vec![]);
        let output = click_text(&mut app, &ctx, "Resolved planning work");
        assert!(text_position(&output, "Archive").is_some(), "{kind}");
        assert!(text_position(&output, "Cancel").is_none(), "{kind}");
        click_text(&mut app, &ctx, "Archive");
        let output = frame(&mut app, &ctx, vec![]);
        assert!(
            text_position(&output, "Resolved planning work").is_none(),
            "{kind}"
        );
        let Screen::Connected(p) = &app.screen else {
            panic!()
        };
        assert!(crate::persistence::archived_tasks::load(&p.chat_slug).contains("CLR-001"));
        assert_eq!(p.state.resolved_items.len(), 1);
    }
}
