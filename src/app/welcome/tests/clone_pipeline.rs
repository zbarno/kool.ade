use super::super::*;
use super::helpers::*;
use crate::core::gitops;
#[test]
fn perform_clone_local_source_feeds_the_full_connect_pipeline() {
    // CHAIN PROOF (ticket plan 5 / AC5): perform_clone →
    // attempt_connect yields a hydrated Project from a PLANNING-LESS
    // source: validation, transaction recovery, state load, scaffold
    // bootstrap (product modules + open-items + config), checkpoint,
    // chat hydration and queue load all run exactly as for
    // a typed path — because it IS the same call. HOME + KOOLADE_HOME
    // point at throwaways for the whole body and are restored on drop;
    // the house shield keeps sibling git spawns on a consistent
    // identity hierarchy.
    let sb = EnvSandbox::enter("chain", false);
    let src = local_source_repo("chain"); // no planning/ — cold start

    let dest =
        perform_clone(src.to_str().unwrap(), "sw-clone").expect("offline local-source clone lands");
    assert_eq!(
        dest,
        sb.home.join("kool-ade-workspaces/sw-clone"),
        "$HOME/kool-ade-workspaces joined with the repo segment verbatim"
    );
    assert!(gitops::is_work_tree(&dest), "the clone is a working tree");
    assert!(dest.join("README.md").exists(), "files arrived");
    no_scratch_left(&sb.home);

    let project = attempt_connect(dest.to_str().unwrap())
        .unwrap_or_else(|e| panic!("the clone must connect like a typed path: {e:?}"));
    assert_eq!(project.state.title, "sw-clone");
    // A new repository receives the modular scaffold directly. Legacy
    // archives are created only when there was an old specification to
    // preserve.
    assert!(
        dest.join(crate::artifacts::product_docs::INDEX).exists(),
        "cold bootstrap landed the product modules"
    );
    assert!(
        !dest.join("planning/specification.md").exists(),
        "cold bootstrap does not create a legacy specification"
    );
    assert!(
        dest.join(crate::artifacts::OPEN_ITEMS_FILE).exists(),
        "bootstrap_missing created the open-items file"
    );
    assert!(
        dest.join(crate::artifacts::CONFIG_FILE).exists(),
        "bootstrap_missing created the config file"
    );
    // Hydration: the welcome line is in memory AND appended to the
    // chat store UNDER the redirected KOOLADE_HOME, keyed by slug.
    assert!(!project.chat.is_empty(), "welcome chat hydrated in memory");
    let slug = crate::persistence::project_slug(&dest);
    let jsonl = crate::persistence::project_dir(&slug).join("chat.jsonl");
    let stored = std::fs::read_to_string(&jsonl).unwrap_or_else(|e| {
        panic!("welcome chat line missing under redirected KOOLADE_HOME ({jsonl:?}): {e}")
    });
    assert!(
        stored.contains("Connected to \u{201c}sw-clone\u{201d}"),
        "welcome line stored: {stored}"
    );
    // Loaded-project population: the git snapshot reflects the bootstrap
    // checkpoint lineage (clean tree, main branch) and the queue loader
    // ran without error.
    assert_eq!(project.git.branch, "main", "snapshot branch populated");
    assert_eq!(
        project.git.dirty, 0,
        "the checkpoint adopted every artifact"
    );
    assert!(
        project.git.last_subject.contains("bootstrap"),
        "last_subject: {}",
        project.git.last_subject
    );
    assert!(project.queue.auto_build, "queue loaded without error");
    let _ = std::fs::remove_dir_all(&src);
}

#[test]
fn migrate_checkpoint_still_stages_tracked_legacy_deletions() {
    // No-regression anchor for the ghost-pathspec filter in
    // attempt_connect: when the legacy spec WAS tracked at HEAD (the
    // everyday evolving-repo shape), the migration checkpoint must
    // still RECORD its deletion and the archive arrival — the filter
    // drops only paths git never knew, never stageable deletions.
    let src = std::env::temp_dir().join(format!("koolade_swclone_tracksrc_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&src);
    std::fs::create_dir_all(src.join("planning")).unwrap();
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(&src)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.name", "Track Src"]);
    git(&["config", "user.email", "track@example.invalid"]);
    git(&["config", "commit.gpgsign", "false"]);
    std::fs::write(src.join("README.md"), "# tracked legacy\n").unwrap();
    std::fs::write(
        src.join("planning/specification.md"),
        crate::artifacts::spec_doc::bootstrap_template("Track Site"),
    )
    .unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "legacy spec tracked at HEAD"]);

    let _sb = EnvSandbox::enter("tracked", false);
    let dest = perform_clone(src.to_str().unwrap(), "sw-track").expect("clone lands");
    let project = attempt_connect(dest.to_str().unwrap())
        .unwrap_or_else(|e| panic!("tracked-legacy connect: {e:?}"));
    assert_eq!(project.state.title, "sw-track");
    assert!(dest.join(crate::artifacts::product_docs::INDEX).exists());
    assert!(
        dest.join(crate::artifacts::product_docs::LEGACY_ARCHIVE)
            .exists()
    );
    assert!(
        dest.join(".koolade-packet/planning/product/index.md")
            .exists()
    );
    assert!(!dest.join("planning/specification.md").exists());

    // Migration checkpoints the deletion + archive arrival before the
    // separate cold-start bootstrap checkpoint adds config and queue files.
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(&dest)
        .args(["log", "-2", "--name-status"])
        .output()
        .unwrap();
    let log_txt = String::from_utf8_lossy(&out.stdout).to_string();
    // Default rename detection may express the move as R(from→archive)
    // rather than D+A — accept either expression; the archive side
    // and the legacy source side must both be present.
    let mut legacy_seen = false;
    let mut archive_seen = false;
    for line in log_txt.lines() {
        if line.starts_with("R") || line.starts_with("D") || line.starts_with("A") {
            if line.split('\t').nth(1) == Some("planning/specification.md") {
                legacy_seen = true;
            }
            if line
                .split('\t')
                .nth(1)
                .is_some_and(|t| t.ends_with("archive/specification-pre-modules.md"))
                || line.contains("planning/archive/specification-pre-modules.md")
            {
                archive_seen = true;
            }
        }
    }
    assert!(legacy_seen, "legacy spec departure staged: {log_txt}");
    assert!(archive_seen, "archive arrival staged: {log_txt}");
    let _ = std::fs::remove_dir_all(&src);
}
