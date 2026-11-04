use super::*;

#[test]
fn no_change_turn_touches_nothing() {
    let (mut st, root, _local_root, _home_guard) = state_at("noop");
    let nt = make_norm(None, None);
    let rc = apply(&mut st, &nt).unwrap();
    assert!(!rc.draft_written && !rc.items_written);
    assert!(rc.changed_store_paths.is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn local_summary_lowercases_agent_summary() {
    let (st, root, _local_root, _home_guard) = state_at("msg");
    let nt = make_norm(Some("Establish initial specification"), None);
    assert_eq!(
        summary::compose(&nt, false, 0, 0, 0),
        "establish initial specification"
    );
    let _ = (st, std::fs::remove_dir_all(&root));
}
