use super::*;

#[test]
fn no_change_turn_touches_nothing() {
    let (mut st, root) = state_at("noop");
    let nt = make_norm(None, None);
    let rc = apply(&mut st, &nt).unwrap();
    assert!(!rc.spec_written && !rc.items_written);
    assert!(rc.repo_relative_paths.is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn unowned_used_category_spawns_single_ownership_item() {
    let (mut st, root) = state_at("own");
    st.config.user = Some(CurrentUser::new("Zach", vec!["Engineering".into()]));
    st.items.push(crate::domain::OpenItem::new(
        "CLR-001".into(),
        Priority::Normal,
        ItemKind::Question,
        "Operations".into(),
        Some("Ops lead".into()),
        "who watches prod logs?".into(),
        String::new(),
    ));
    st.baseline_items_md = items_io::serialize(&st.items);
    let e = TurnEnvelope {
        schema_version: Some(2),
        assistant_message: Some("logged".into()),
        change_summary: None,
        document_updates: None,
        planning_tasks: None,
        updated_specification: None,
        open_items_added: None,
        open_items_updated: None,
        open_items_resolved: None,
        next_question_id: None,
        requested_action: None,
        follow_up_task: None,
        interview: None,
        task_stories: None,
        task_outline: None,
        plans: None,
        recommendation: None,
        plan_remediation: None,
    };
    let nt = validation::validate(&e, &st, &st.effective_user()).unwrap();
    let rc = apply(&mut st, &nt).unwrap();
    // Only the USED category (Operations) spawns an ownership item, and
    // exactly one; a re-apply must not multiply them.
    let n_own = st
        .items
        .iter()
        .filter(|i| i.kind == ItemKind::Ownership)
        .count();
    assert_eq!(n_own, 1);
    assert_eq!(rc.synthesized_open_items.len(), 1);
    let nt2 = validation::validate(&e, &st, &st.effective_user()).unwrap();
    let rc2 = apply(&mut st, &nt2).unwrap();
    assert!(rc2.synthesized_open_items.is_empty());
    assert_eq!(
        st.items
            .iter()
            .filter(|i| i.kind == ItemKind::Ownership)
            .count(),
        1
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn commit_phrase_lowercases_agent_summary() {
    let (st, root) = state_at("msg");
    let nt = make_norm(Some("Establish initial specification"), None);
    assert_eq!(
        commit_message::compose(&nt, true, false, 0, 0, 0),
        "planner: establish initial specification"
    );
    let _ = (st, std::fs::remove_dir_all(&root));
}
