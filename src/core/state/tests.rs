use super::*;
use crate::core::gitops;
use crate::domain::{CurrentUser, GUEST_NAME, IdentitySource};

fn mkrepo_without_git(prefix: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("koolade_state_{prefix}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn mkrepo(prefix: &str) -> PathBuf {
    let p = mkrepo_without_git(prefix);
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&p)
            .status()
            .unwrap()
            .success()
    );
    p
}

/// Git working tree with (optionally) a local identity plus a seeded
/// Canonical project config's Current User block — mirrors the
/// `gitops::tests` temp-repo recipe.
fn git_fixture(prefix: &str, local_name: Option<&str>) -> PathBuf {
    let p = mkrepo(prefix);
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(&p)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    };
    git(&["init", "-q", "-b", "main"]);
    if let Some(name) = local_name {
        git(&["config", "user.name", name]);
    }
    git(&["config", "user.email", "developer@example.test"]);
    let dest = crate::artifacts::repo_artifact(&p, CONFIG_FILE);
    std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
    std::fs::write(
            &dest,
            "# Planner Configuration\n\n## Current User\nName: Bob\nGroups: Ops\n\n## Stakeholders\n\n### QA\n(no owner configured)\n",
        )
        .unwrap();
    p
}

fn config_path(p: &Path) -> PathBuf {
    crate::artifacts::repo_artifact(p, CONFIG_FILE)
}

#[test]
fn bootstraps_skeletons_and_reports_creations() {
    let repo = mkrepo("boot");
    let mut st = PlannerState::load(&repo).unwrap();
    let created = st.bootstrap_missing().unwrap();
    assert!(created.contains(&crate::artifacts::SPEC_FILE.to_owned()));
    assert!(created.contains(&OPEN_ITEMS_FILE.to_owned()));
    assert!(created.contains(&CONFIG_FILE.to_owned()));
    assert!(created.contains(&crate::core::project_repos::PROJECT_FILE.to_owned()));
    assert_eq!(created.len(), 11);
    // Second call creates nothing.
    let created2 = st.bootstrap_missing().unwrap();
    assert!(created2.is_empty());
    let _ = std::fs::remove_dir_all(&repo);
}

#[test]
fn concurrent_bootstrap_rejects_the_stale_initializer_without_overwriting() {
    let repo = mkrepo("concurrent_boot");
    let mut first = PlannerState::load(&repo).unwrap();
    let mut stale = PlannerState::load(&repo).unwrap();

    first.bootstrap_missing().unwrap();
    let before_items =
        std::fs::read(repo.join(crate::artifacts::layout::canonical::OPEN_ITEMS)).unwrap();
    let before_config =
        std::fs::read(repo.join(crate::artifacts::layout::canonical::PROJECT_CONFIG)).unwrap();
    let error = stale.bootstrap_missing().unwrap_err();

    assert!(error.to_string().contains("planning store changed"));
    assert_eq!(
        std::fs::read(repo.join(crate::artifacts::layout::canonical::OPEN_ITEMS)).unwrap(),
        before_items
    );
    assert_eq!(
        std::fs::read(repo.join(crate::artifacts::layout::canonical::PROJECT_CONFIG)).unwrap(),
        before_config
    );
    let _ = std::fs::remove_dir_all(&repo);
}

#[path = "tests/managed_store.rs"]
mod managed_store;

#[path = "tests/managed_store_equivalence.rs"]
mod managed_store_equivalence;

#[test]
fn seeded_config_contains_starter_categories() {
    let repo = mkrepo("seed");
    let mut st = PlannerState::load(&repo).unwrap();
    st.config.user = Some(CurrentUser::new("Sam", vec!["QA".into()]));
    st.bootstrap_missing().unwrap();
    let text =
        crate::artifacts::read_utf8_lossy(&crate::artifacts::repo_artifact(&repo, CONFIG_FILE))
            .unwrap();
    assert!(text.contains("Name: Sam"));
    assert!(text.contains("### InfoSec"));
    let _ = std::fs::remove_dir_all(&repo);
}

/// Full FR-13 descent under a shielded ambient hierarchy (the dev
/// machine's GLOBAL config carries its own identity and must not leak
/// into fixture expectations).
#[test]
fn git_identity_seats_operator_and_survives_resync_descent() {
    let _shield = gitops::test_support::shield("state-prio");
    let repo = git_fixture("prio", Some("Alex Developer"));
    let block_before = std::fs::read_to_string(config_path(&repo)).unwrap();

    let mut st = PlannerState::load(&repo).unwrap();
    // git user.name wins over the contradicting config block…
    assert_eq!(st.identity.user.name, "Alex Developer");
    assert_eq!(st.identity.source, IdentitySource::GitUserName);
    assert_eq!(st.effective_user().name, "Alex Developer");
    // …while config still contributes the groups git cannot express.
    assert_eq!(st.effective_user().groups, vec!["Ops".to_string()]);
    // …and never writes to the config block on the way.
    assert_eq!(
        std::fs::read_to_string(config_path(&repo)).unwrap(),
        block_before,
        "identity probing must leave config.md byte-intact"
    );

    // Rewriting the block name demotes nothing: git still outranks it.
    let p = config_path(&repo);
    std::fs::write(&p, block_before.replace("Name: Bob", "Name: Carol")).unwrap();
    st.resync().unwrap();
    assert_eq!(st.identity.user.name, "Alex Developer");
    assert_eq!(st.identity.source, IdentitySource::GitUserName);

    // Unset name (ambient shadowed by the shield) → email fallback.
    let _ = std::process::Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["config", "--unset", "user.name"])
        .output()
        .unwrap();
    st.resync().unwrap();
    assert_eq!(st.identity.user.name, "developer@example.test");
    assert_eq!(st.identity.source, IdentitySource::GitUserEmail);
    assert_eq!(st.effective_user().groups, vec!["Ops".to_string()]);

    // Unset email too → the config block finally seats the operator.
    let _ = std::process::Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["config", "--unset", "user.email"])
        .output()
        .unwrap();
    st.resync().unwrap();
    assert_eq!(st.identity.user.name, "Carol");
    assert_eq!(st.identity.source, IdentitySource::ConfigBlock);
    assert_eq!(st.effective_user().groups, vec!["Ops".to_string()]);

    // An external edit of the local git user.name takes effect on the
    // next resync — derivation is not one-shot at connect.
    let _ = std::process::Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["config", "user.name", "Mira Chen"])
        .output()
        .unwrap();
    st.resync().unwrap();
    assert_eq!(st.identity.user.name, "Mira Chen");
    assert_eq!(st.identity.source, IdentitySource::GitUserName);
    let _ = std::fs::remove_dir_all(&repo);
}

/// Degradation contract (§9): a non-git tree raises NO error from the
/// identity probes and simply falls to config-else-guest.
#[test]
fn gitless_tree_degrades_to_config_block_then_guest() {
    // Config block only (no .git anywhere) → ConfigBlock seat.
    let repo = mkrepo_without_git("cfgonly");
    let config = crate::artifacts::layout::ArtifactLayout::new(&repo).config_root();
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(
            config.join("project.md"),
            "# Planner Configuration\n\n## Current User\nName: Dana\nGroups: Ops, Platform\n\n## Stakeholders\n\n### QA\n(no owner configured)\n",
        ).unwrap();
    let st = PlannerState::load(&repo).unwrap();
    assert_eq!(st.identity.user.name, "Dana");
    assert_eq!(
        st.identity.user.groups,
        vec!["Ops".to_string(), "Platform".to_string()]
    );
    assert_eq!(st.identity.source, IdentitySource::ConfigBlock);
    assert_eq!(st.effective_user().name, "Dana");
    let _ = std::fs::remove_dir_all(&repo);

    // No git, no artifacts → guest, and load STILL SUCCEEDS.
    let bare = mkrepo_without_git("guest");
    let st = PlannerState::load(&bare).unwrap();
    assert_eq!(st.identity.user.name, GUEST_NAME);
    assert_eq!(st.identity.source, IdentitySource::Guest);
    assert_eq!(st.effective_user().name, GUEST_NAME);
    let _ = std::fs::remove_dir_all(&bare);
}

/// AC2 (literal): `user.name` unset (ambient shadowed by the shield),
/// `user.email` set, and a config block that lists groups — the EMAIL
/// seats the operator and the groups come exclusively from the block.
#[test]
fn email_fallback_seats_when_name_unset_and_ambient_shadowed() {
    let _shield = gitops::test_support::shield("state-ac2");
    let repo = git_fixture("ac2", None); // local identity: email only
    let _ = std::process::Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["config", "user.email", "eve@example.org"])
        .output()
        .unwrap();
    let st = PlannerState::load(&repo).unwrap();
    assert_eq!(st.identity.user.name, "eve@example.org");
    assert_eq!(st.identity.source, IdentitySource::GitUserEmail);
    assert_eq!(st.effective_user().name, "eve@example.org");
    assert_eq!(st.effective_user().groups, vec!["Ops".to_string()]);
    let _ = std::fs::remove_dir_all(&repo);
}

/// AC1-shaped fixture: local git identity 'Ada Lovelace' plus a
/// contradicting 'Bob' block — git wins, block byte-intact.
#[test]
fn ada_lovelace_git_beats_bob_config() {
    let _shield = gitops::test_support::shield("state-ada");
    let repo = git_fixture("ada", Some("Ada Lovelace"));
    let _ = std::process::Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["config", "user.email", "ada@example.org"])
        .output()
        .unwrap();
    let block = std::fs::read_to_string(config_path(&repo)).unwrap();
    let st = PlannerState::load(&repo).unwrap();
    assert_eq!(st.identity.user.name, "Ada Lovelace");
    assert_eq!(st.identity.source, IdentitySource::GitUserName);
    assert_eq!(st.effective_user().name, "Ada Lovelace");
    assert_eq!(st.config.user.as_ref().unwrap().name, "Bob");
    assert_eq!(std::fs::read_to_string(config_path(&repo)).unwrap(), block);
    let _ = std::fs::remove_dir_all(&repo);
}
