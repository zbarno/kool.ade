use super::*;

/// AC5 (literal chain): the display caches (panel highlighting, chat
/// eligibility) report the GIT-DERIVED seat from `effective_user` — not
/// the config block's contradicting name and not the project title. Then
/// an EXTERNAL git edit of local `user.name` → a settings Save (which
/// resyncs) → the refreshed display cache reports the NEW seat — proving
/// derivation runs after settings-Save, not only at connect.
#[test]
fn derive_caches_projects_the_git_derived_seat_not_third_identities() {
    let _shield = crate::core::gitops::test_support::shield("caches-mira");
    let root = std::env::temp_dir().join(format!("koolade_caches_mira_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let git = |args: &[&str]| -> std::process::Output {
        std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(args)
            .output()
            .unwrap()
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.name", "Sam Lee"]);
    git(&["config", "user.email", "sam@example.org"]);
    let config = root.join(".koolade-packet/config");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(
        config.join("project.md"),
        "# Planner Configuration\n\n## Current User\nName: Bob\nGroups: Ops\n\n## Stakeholders\n\n### QA\n(no owner configured)\n",
    )
    .unwrap();

    let state = crate::core::state::PlannerState::load(&root).unwrap();
    let mut proj = Project {
        task_chats: Default::default(),
        activity: Default::default(),
        state,
        chat_slug: "test-slug".into(),
        chat: Vec::new(),
        draft: String::new(),
        queue: Default::default(),
        queue_lock: None,
        active_implementations: Default::default(),
        pr_refresh: None,
        reconciliation: Default::default(),
        investigation: None,
        investigation_attempted: Default::default(),
        investigation_cooldown_until: None,
        last_pr_refresh: None,
        implementation_states: Default::default(),
        active_turn: None,
        task_turns: Default::default(),
        task_live: Default::default(),
        planning_work: Default::default(),
        active_planning_work: None,
        live_progress: Default::default(),
        next_question_id: None,
        git: Default::default(),
        task_documents: Vec::new(),
        archived_tasks: Default::default(),
        cancelled_work: Default::default(),
    };
    // Connect: the git seat is projected — not the block's 'Bob',
    // not the project title.
    let (cached, _eligible) = KooladeApp::derive_caches(&proj);
    assert_eq!(cached.name, "Sam Lee");
    assert_eq!(cached.groups, vec!["Ops".to_string()]);
    assert_eq!(
        proj.state.identity.source,
        crate::domain::user::IdentitySource::GitUserName
    );

    // External edit of the local git identity takes effect only on the
    // next resync — the cache is still stale before the save.
    git(&["config", "user.name", "Mira Chen"]);
    let (stale, _eligible) = KooladeApp::derive_caches(&proj);
    assert_eq!(stale.name, "Sam Lee");

    // AC5: a no-edit settings Save (write → resync → checkpoint) re-
    // derives the seat, and the refreshed display cache follows.
    let mut dlg = crate::app::dialogs::DlgSettings::from_project(&proj);
    assert_eq!(dlg.user_name, "Sam Lee");
    dlg.apply(&mut proj)
        .expect("no-edit settings save must succeed");
    let (resynced, _eligible) = KooladeApp::derive_caches(&proj);
    assert_eq!(resynced.name, "Mira Chen");
    assert_eq!(
        proj.state.identity.source,
        crate::domain::user::IdentitySource::GitUserName
    );
    let log = String::from_utf8_lossy(&git(&["log", "-1", "--pretty=%s"]).stdout).into_owned();
    assert!(
        log.contains("settings: update workspace settings"),
        "checkpoint subject: {log}"
    );
    let _ = std::fs::remove_dir_all(&root);
}
