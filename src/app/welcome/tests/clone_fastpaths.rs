use super::super::*;
use super::helpers::*;
use crate::core::gitops;
use crate::error::AppError;
#[test]
fn perform_clone_healthy_existing_tree_fast_paths_without_retouching() {
    // AC6: $HOME/kool-ade-workspaces/{repo} already a HEALTHY work tree from a previous
    // clone → immediate return, no re-clone, no scratch remnant, and
    // not even a new commit on the tree.
    let home = scratch_home("health");
    let src = local_source_repo("health");
    let dest = home.join("kool-ade-workspaces/sw-health");
    std::fs::create_dir_all(&dest).unwrap();
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(&dest)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.name", "Health"]);
    git(&["config", "user.email", "h@example.invalid"]);
    std::fs::write(dest.join("note.txt"), b"frozen-bytes").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "healthy head"]);

    let got =
        perform_clone_at(src.to_str().unwrap(), "sw-health", &home).expect("fast path returns");
    assert_eq!(got, dest);
    assert_eq!(
        std::fs::read(dest.join("note.txt")).unwrap(),
        b"frozen-bytes",
        "no re-clone: tree byte-identical"
    );
    assert!(gitops::is_work_tree(&dest));
    let head = std::process::Command::new("git")
        .arg("-C")
        .arg(&dest)
        .args(["log", "-1", "--format=%s"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&head.stdout),
        "healthy head\n",
        "no checkpoint of any kind was added"
    );
    no_scratch_left(&home);
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&src);
}

#[test]
fn perform_clone_plain_repo_connects_identically_to_a_typed_path() {
    // AC5'S identity clause made observable: the SAME planning-less
    // tree, reached once THROUGH the clone and once by hand-typing
    // the source's path, completes with side-effect parity — both
    // bootstrap the scaffold and checkpoint it in both paths, both
    // hydrate chat. The clone feeds EXACTLY today's connect path
    // because it literally is that call.
    let _sb = EnvSandbox::enter("plain", false);
    let src = local_source_repo("plain-src"); // planning-less
    let src_title = src.file_name().unwrap().to_str().unwrap().to_string();

    // Clone leg first, while `src` is still pristine.
    let dest =
        perform_clone(src.to_str().unwrap(), "sw-plain").expect("offline local-source clone lands");
    assert!(gitops::is_work_tree(&dest));
    let proj_clone = attempt_connect(dest.to_str().unwrap())
        .unwrap_or_else(|e| panic!("cloned planning-less repo must connect: {e:?}"));

    // Typed leg: the operator opens the SAME source by hand.
    let proj_typed = attempt_connect(src.to_str().unwrap())
        .unwrap_or_else(|e| panic!("typed planning-less repo must connect identically: {e:?}"));

    assert_eq!(proj_clone.state.title, "sw-plain");
    assert_eq!(
        proj_typed.state.title, src_title,
        "titles follow the dir names"
    );
    for (root, what) in [(dest.as_path(), "clone leg"), (src.as_path(), "typed leg")] {
        assert!(
            root.join(crate::artifacts::product_docs::INDEX).exists(),
            "{what}: product modules landed"
        );
        assert!(
            !root.join("planning/specification.md").exists(),
            "{what}: cold bootstrap does not create a legacy spec"
        );
        assert!(
            !root.join(crate::artifacts::OPEN_ITEMS_FILE).exists(),
            "{what}: an empty queue uses no monolithic file"
        );
        assert!(
            root.join(crate::artifacts::CONFIG_FILE).exists(),
            "{what}: config bootstrapped"
        );
    }
    assert!(
        proj_clone
            .chat
            .last()
            .is_some_and(|m| m.text.contains("Connected to")),
        "clone leg hydrated the welcome line"
    );
    assert!(
        proj_typed
            .chat
            .last()
            .is_some_and(|m| m.text.contains("Connected to")),
        "typed leg hydrated the same welcome line"
    );
    let _ = std::fs::remove_dir_all(&src);
}

#[test]
fn perform_clone_preexisting_directory_fast_paths_and_connect_diagnoses_it() {
    let home = scratch_home("skip");
    let src = local_source_repo("skip");
    let dest = home.join("kool-ade-workspaces/sw-skip");
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(dest.join("marker.bin"), b"frozen-bytes").unwrap();

    let got = perform_clone_at(src.to_str().unwrap(), "sw-skip", &home).expect("fast path returns");
    assert_eq!(got, dest);
    assert_eq!(
        std::fs::read(dest.join("marker.bin")).unwrap(),
        b"frozen-bytes",
        "no re-clone: the pre-existing directory is byte-identical"
    );
    no_scratch_left(&home);

    // A FOREIGN (non-git) directory is diagnosed by the INCUMBENT
    // attempt_connect banner — no invented new logic.
    match attempt_connect(dest.to_str().unwrap()) {
        Err(err) => {
            let msg = format!("{} | {}", err.headline(), err.detail());
            assert!(
                msg.contains("no .git directory found"),
                "legacy banner: {msg}"
            );
        }
        Ok(_) => panic!("a foreign (non-git) directory must be rejected"),
    }
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&src);
}

#[test]
fn perform_clone_sweeps_stale_scratch_before_cloning() {
    let home = scratch_home("sweep");
    let src = local_source_repo("sweep");
    let stale = home.join("kool-ade-workspaces/sw-sweep.koolade-cloning.999");
    std::fs::create_dir_all(stale.join("junk")).unwrap();
    std::fs::write(stale.join("junk/leftover.txt"), "dead").unwrap();

    let dest = perform_clone_at(src.to_str().unwrap(), "sw-sweep", &home).expect("clone lands");
    assert!(!stale.exists(), "stale foreign-pid scratch swept");
    assert!(gitops::is_work_tree(&dest));

    // Same-pid repetition: the sweep must also clear THIS process'
    // scratch name, and the second clone rides the same scratch path.
    let own = home.join(format!(
        "kool-ade-workspaces/sw-sweep.koolade-cloning.{}",
        std::process::id()
    ));
    std::fs::remove_dir_all(&dest).unwrap();
    std::fs::create_dir_all(&own).unwrap();
    let again = perform_clone_at(src.to_str().unwrap(), "sw-sweep", &home).expect("reclone lands");
    assert!(!own.exists(), "same-pid scratch swept");
    assert!(again.is_dir());
    no_scratch_left(&home);
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&src);
}

#[test]
fn perform_clone_destination_occupied_by_a_file_is_invalid_repo() {
    let home = scratch_home("fileocc");
    let src = local_source_repo("fileocc");
    std::fs::create_dir_all(home.join("kool-ade-workspaces")).unwrap();
    std::fs::write(home.join("kool-ade-workspaces/sw-fileocc"), "blocker").unwrap();

    match perform_clone_at(src.to_str().unwrap(), "sw-fileocc", &home) {
        Err(AppError::InvalidRepo { path, detail }) => {
            assert!(path.ends_with("sw-fileocc"));
            assert!(
                detail.contains("a file already occupies that name"),
                "{detail}"
            );
        }
        other => panic!("expected InvalidRepo, got {other:?}"),
    }
    assert!(
        home.join("kool-ade-workspaces/sw-fileocc").is_file(),
        "the blocker file survived"
    );
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&src);
}

#[test]
fn perform_clone_without_home_errors_locate_home() {
    let _sb = EnvSandbox::enter("nohome", true); // HOME removed for the body
    match perform_clone("https://github.com/o/r.git", "sw-nohome") {
        Err(AppError::Io { op, detail }) => {
            assert!(op.contains("locate home directory"), "{op}");
            assert!(detail.to_lowercase().contains("$home"), "{detail}");
        }
        other => panic!("expected Io locate-home, got {other:?}"),
    }
}
