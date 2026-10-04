use super::super::*;
use super::support::{project_from, tempdir};
use crate::core::gitops::test_support;
use crate::domain::user::IdentitySource;
#[test]
fn settings_echoes_derived_git_identity_not_the_config_declaration() {
    let _shield = test_support::shield("dlg-echo");
    let root = tempdir("ada");
    let git = |args: &[&str]| -> std::process::Output {
        std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(args)
            .output()
            .unwrap()
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.name", "Ada Lovelace"]);
    git(&["config", "user.email", "ada@example.org"]);
    let config_path = crate::artifacts::repo_artifact(&root, CONFIG_FILE);
    std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
    std::fs::write(
        &config_path,
        "# Planner Configuration\n\n## Current User\nName: Bob\nGroups:\n\n## Stakeholders\n\n### QA\n(no owner configured)\n",
    )
    .unwrap();

    let proj = project_from(&root);
    assert_eq!(proj.state.identity.user.name, "Ada Lovelace");
    assert_eq!(proj.state.identity.source, IdentitySource::GitUserName);
    let dlg = DlgSettings::from_project(&proj);
    // The DERIVED value, not the config's 'Bob'.
    assert_eq!(dlg.user_name, "Ada Lovelace");
    assert!(
        dlg.identity_note.contains("git user.name"),
        "note: {}",
        dlg.identity_note
    );
    assert!(
        dlg.identity_note.contains("Ada Lovelace"),
        "note: {}",
        dlg.identity_note
    );
    assert!(
        !dlg.identity_note.contains("override"),
        "git seat is not an override: {}",
        dlg.identity_note
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// Config-block seat: the echo follows the block and the note flags the
/// override character of the fields.
#[test]
fn settings_echo_config_block_seat_flags_override() {
    let root = tempdir("dana");
    let config_path = crate::artifacts::repo_artifact(&root, CONFIG_FILE);
    std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
    std::fs::write(
        &config_path,
        "# Planner Configuration\n\n## Current User\nName: Dana\nGroups: Ops, Platform\n\n## Stakeholders\n\n### QA\n(no owner configured)\n",
    )
    .unwrap();
    let proj = project_from(&root);
    let dlg = DlgSettings::from_project(&proj);
    assert_eq!(dlg.user_name, "Dana");
    assert_eq!(dlg.user_groups, "Ops, Platform");
    assert!(
        dlg.identity_note
            .contains(".koolade-packet/config/project.md"),
        "note: {}",
        dlg.identity_note
    );
    assert!(
        dlg.identity_note.contains("override"),
        "note: {}",
        dlg.identity_note
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// Legacy degradation: git-less, artifact-less tree → guest echo with the
/// matching source label; the dialog still builds cleanly. A GHOSTED
/// block (whitespace-only Name:, real Groups) is nulled by the tolerant
/// parser at load (pre-existing behaviour), so the dialog echoes blanks
/// and a no-edit Save round-trips exactly what the app believes.
#[test]
fn settings_echo_guest_when_nothing_identifies_the_operator() {
    let root = tempdir("guest");
    let proj = project_from(&root);
    let dlg = DlgSettings::from_project(&proj);
    // Fields stay BLANK for a guest seat (legacy echo for an unset
    // block) so a no-edit Save cannot persist a phantom `Name: (guest)`
    // declaration; the provenance line carries the guest notice.
    assert_eq!(dlg.user_name, "");
    assert!(dlg.user_groups.is_empty());
    assert!(
        dlg.identity_note.contains("guest"),
        "note: {}",
        dlg.identity_note
    );
    assert!(
        dlg.identity_note.contains("override"),
        "note: {}",
        dlg.identity_note
    );
    let _ = std::fs::remove_dir_all(&root);

    // Ghosted block (whitespace-only Name:, real Groups): the tolerant
    // parser nulls the whole block on load (pre-existing behaviour), so
    // the dialog echoes blanks — and a no-edit Save round-trips exactly
    // what the app believes, losing nothing it holds.
    let ghost = tempdir("ghost");
    let config_path = crate::artifacts::repo_artifact(&ghost, CONFIG_FILE);
    std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
    std::fs::write(
        config_path,
        "# Planner Configuration\n\n## Current User\nName:    \nGroups: Ops\n\n## Stakeholders\n\n### QA\n(no owner configured)\n",
    )
    .unwrap();
    let proj = project_from(&ghost);
    assert_eq!(proj.state.identity.source, IdentitySource::Guest);
    assert!(
        proj.state.config.user.is_none(),
        "tolerant parser nulls ghosted blocks"
    );
    let dlg = DlgSettings::from_project(&proj);
    assert_eq!(dlg.user_name, "");
    assert_eq!(dlg.user_groups, "");
    let _ = std::fs::remove_dir_all(&ghost);
}

/// AC4 save round-trip on a DRIFTED tree (git 'Ada Lovelace' vs config
/// 'Bob'): a no-edit Save writes the echoed derived identity into the
/// block, re-derives (git still wins — saving can never downgrade a
/// git-derived seat), checkpoints with the conventional settings subject,
/// and the reopened dialog echoes the SAME derived identity.
#[test]
fn no_edit_save_on_drifted_git_tree_keeps_git_seat_and_sets_checkpoint() {
    let _shield = test_support::shield("dlg-save");
    let root = tempdir("adasave");
    let git = |args: &[&str]| -> std::process::Output {
        std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(args)
            .output()
            .unwrap()
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.name", "Ada Lovelace"]);
    git(&["config", "user.email", "ada@example.org"]);
    let config_path = crate::artifacts::repo_artifact(&root, CONFIG_FILE);
    std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
    std::fs::write(
        &config_path,
        "# Planner Configuration\n\n## Current User\nName: Bob\nGroups: Ops\n\n## Stakeholders\n\n### QA\n(no owner configured)\n",
    )
    .unwrap();

    let mut proj = project_from(&root);
    assert_eq!(proj.state.identity.source, IdentitySource::GitUserName);
    let mut dlg = DlgSettings::from_project(&proj);
    // The card prefills the DERIVED name; the operator hits Save as-is.
    assert_eq!(dlg.user_name, "Ada Lovelace");
    let short = dlg.apply(&mut proj).expect("no-edit save must succeed");
    assert_eq!(short.chars().count(), 7);

    // Resync re-derived: the git seat survived the block rewrite.
    assert_eq!(proj.state.identity.user.name, "Ada Lovelace");
    assert_eq!(proj.state.identity.source, IdentitySource::GitUserName);
    assert_eq!(proj.state.effective_user().name, "Ada Lovelace");
    let log = String::from_utf8_lossy(&git(&["log", "-1", "--pretty=%s"]).stdout).into_owned();
    assert!(
        log.contains("settings: update workspace settings"),
        "checkpoint subject: {log}"
    );
    // Storing the echoed git name in the block is benign redundancy.
    let block = std::fs::read_to_string(&config_path).unwrap();
    assert!(block.contains("Name: Ada Lovelace"), "block: {block}");

    // Reopening the dialog echoes the same derived identity/provenance.
    let again = DlgSettings::from_project(&proj);
    assert_eq!(again.user_name, "Ada Lovelace");
    assert!(
        again.identity_note.contains("git user.name"),
        "note: {}",
        again.identity_note
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn assigning_category_owner_resolves_ownership_gap_with_a_checkpoint() {
    let root = tempdir("ownership-resolve");
    let git = |args: &[&str]| -> std::process::Output {
        std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(args)
            .output()
            .unwrap()
    };
    for args in [
        vec!["init", "-q", "-b", "main"],
        vec!["config", "user.name", "Settings Test"],
        vec!["config", "user.email", "settings@example.invalid"],
    ] {
        assert!(git(&args).status.success());
    }
    let config_path = crate::artifacts::repo_artifact(&root, CONFIG_FILE);
    std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
    std::fs::write(
        &config_path,
        "# Planner Configuration\n\n## Current User\nName: Settings Test\nGroups:\n\n## Stakeholders\n\n### Security\n(no owner configured)\n",
    )
    .unwrap();
    let gap = crate::domain::OpenItem::new(
        "CLR-001".into(),
        crate::domain::Priority::High,
        crate::domain::ItemKind::Ownership,
        "Security".into(),
        None,
        "Who owns Security?".into(),
        "Security questions need an assigned stakeholder.".into(),
    );
    let question = crate::domain::OpenItem::new(
        "CLR-002".into(),
        crate::domain::Priority::Normal,
        crate::domain::ItemKind::Question,
        "Security".into(),
        None,
        "Which audit log should be retained?".into(),
        "This decision remains unanswered.".into(),
    );
    let derived_gap_question = crate::domain::OpenItem::new(
        "CLR-003".into(),
        crate::domain::Priority::Normal,
        crate::domain::ItemKind::Question,
        "QA".into(),
        None,
        "Which test report should be retained?".into(),
        "This decision remains unanswered.".into(),
    );
    let open_items =
        crate::artifacts::repo_artifact(&root, crate::artifacts::layout::canonical::OPEN_ITEMS);
    std::fs::create_dir_all(open_items.parent().unwrap()).unwrap();
    std::fs::write(
        &open_items,
        crate::artifacts::items_io::serialize(&[gap, question, derived_gap_question]),
    )
    .unwrap();

    let mut project = project_from(&root);
    let mut dialog = DlgSettings::from_project(&project);
    dialog
        .rows
        .iter_mut()
        .find(|row| row.category == "Security")
        .unwrap()
        .members = "Morgan".into();
    dialog.rows.push(Row {
        category: "QA".into(),
        members: "Riley".into(),
    });
    dialog.apply(&mut project).unwrap();

    assert_eq!(project.state.items.len(), 2);
    assert_eq!(project.state.items[0].id, "CLR-002");
    assert_eq!(project.state.items[1].id, "CLR-003");
    assert_eq!(project.state.resolved_items.len(), 2);
    let persisted_gap = project
        .state
        .resolved_items
        .iter()
        .find(|item| item.id == "CLR-001")
        .unwrap();
    let derived_gap = project
        .state
        .resolved_items
        .iter()
        .find(|item| item.category == "QA")
        .unwrap();
    for item in [persisted_gap, derived_gap] {
        assert_eq!(item.status, crate::domain::ItemStatus::Resolved);
    }
    assert!(persisted_gap.evidence.contains("assigned to Morgan"));
    assert!(derived_gap.evidence.contains("assigned to Riley"));
    assert!(derived_gap.id.starts_with("ownership:"));
    let persisted = crate::core::state::PlannerState::load(&root).unwrap();
    assert_eq!(persisted.items.len(), 2);
    assert_eq!(persisted.resolved_items.len(), 2);

    let _ = std::fs::remove_dir_all(&root);
}
