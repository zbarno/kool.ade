use super::super::*;
use super::support::{
    isolate_global_git_config, planning_store_for, project_from, remove_fixture, tempdir,
};
use crate::core::gitops::test_support;
use crate::domain::user::IdentitySource;
#[test]
fn settings_echoes_derived_git_identity_not_the_config_declaration() {
    let _shield = test_support::shield("dlg-echo");
    let root = tempdir("ada");
    let _git_config = isolate_global_git_config(&root);
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
    let store = planning_store_for(&root);
    let config_path = crate::artifacts::repo_artifact(&store, CONFIG_FILE);
    std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
    std::fs::write(
        &config_path,
        "# Local Application Settings\n\n## Current User\nName: Bob\n",
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
    remove_fixture(&root);
}

/// Local-user seat: the echo follows the private application setting.
#[test]
fn settings_echo_config_block_seat_flags_override() {
    let root = tempdir("dana");
    let _git_config = isolate_global_git_config(&root);
    let store = planning_store_for(&root);
    let config_path = crate::artifacts::repo_artifact(&store, CONFIG_FILE);
    std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
    std::fs::write(
        &config_path,
        "# Local Application Settings\n\n## Current User\nName: Dana\n",
    )
    .unwrap();
    let proj = project_from(&root);
    let dlg = DlgSettings::from_project(&proj);
    assert_eq!(dlg.user_name, "Dana");
    assert!(
        dlg.identity_note.contains("local user setting"),
        "note: {}",
        dlg.identity_note
    );
    assert!(
        !dlg.identity_note.contains("override"),
        "local user identity is not an override: {}",
        dlg.identity_note
    );
    remove_fixture(&root);
}

/// A git-less, artifact-less tree still has a stable local guest identity.
#[test]
fn settings_echo_guest_when_nothing_identifies_the_operator() {
    let root = tempdir("guest");
    let _git_config = isolate_global_git_config(&root);
    let proj = project_from(&root);
    let dlg = DlgSettings::from_project(&proj);
    // Fields stay BLANK for a guest seat (legacy echo for an unset
    // block) so a no-edit Save cannot persist a phantom `Name: (guest)`
    // declaration; the provenance line carries the guest notice.
    assert_eq!(dlg.user_name, "");
    assert!(
        dlg.identity_note.contains("guest"),
        "note: {}",
        dlg.identity_note
    );
    remove_fixture(&root);
}

/// AC4 save round-trip on a DRIFTED tree (git 'Ada Lovelace' vs config
/// 'Bob'): a no-edit Save writes the echoed derived identity into the
/// private block, re-derives (git still wins — saving can never downgrade a
/// git-derived seat), and the reopened dialog echoes the SAME derived identity
/// without creating a commit in the code repository.
#[test]
fn no_edit_save_on_drifted_git_tree_keeps_git_seat_without_git_commit() {
    let _shield = test_support::shield("dlg-save");
    let root = tempdir("adasave");
    let _git_config = isolate_global_git_config(&root);
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
    let store = planning_store_for(&root);
    let config_path = crate::artifacts::repo_artifact(&store, CONFIG_FILE);
    std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
    std::fs::write(
        &config_path,
        "# Local Application Settings\n\n## Current User\nName: Bob\n",
    )
    .unwrap();

    let mut proj = project_from(&root);
    let initial_head = git(&["rev-parse", "HEAD"]);
    assert!(initial_head.status.success());
    let initial_head = String::from_utf8(initial_head.stdout)
        .unwrap()
        .trim()
        .to_owned();
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
    let current_head = git(&["rev-parse", "HEAD"]);
    assert!(current_head.status.success());
    assert_eq!(
        String::from_utf8(current_head.stdout).unwrap().trim(),
        initial_head
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
    remove_fixture(&root);
}
