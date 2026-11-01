use super::super::*;
use super::helpers::*;
use crate::core::gitops;
use crate::error::AppError;
#[test]
fn perform_clone_refuses_to_adopt_an_existing_same_origin_checkout() {
    // Existing folders are never accepted as managed clones, even when their
    // Git origin matches. Reopening uses a saved workspace association.
    let home = scratch_home("health");
    let src = local_source_repo("health");
    let dest = home.join("kool-ade-workspaces/sw-health");
    std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
    let output = std::process::Command::new("git")
        .args(["clone", "--quiet"])
        .arg(&src)
        .arg(&dest)
        .output()
        .unwrap();
    assert!(output.status.success(), "local clone fixture failed");
    std::fs::write(dest.join("note.txt"), b"frozen-bytes").unwrap();

    match perform_clone_at(src.to_str().unwrap(), "sw-health", &home) {
        Err(AppError::InvalidRepo { detail, .. }) => {
            assert!(
                detail.contains("will not adopt an existing checkout"),
                "{detail}"
            );
        }
        other => panic!("expected existing checkout refusal, got {other:?}"),
    }
    assert!(gitops::is_work_tree(&dest));
    assert_eq!(
        std::fs::read(dest.join("note.txt")).unwrap(),
        b"frozen-bytes"
    );
    no_scratch_left(&home);
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&src);
}

#[test]
fn perform_clone_plain_repo_meets_the_same_planning_setup_gate_as_a_typed_path() {
    // Identity clause under the enforced model: the SAME planning-less
    // tree, reached once THROUGH the clone and once by hand-typing the
    // source's path, is refused with the same actionable planning-setup
    // guidance — the clone feeds EXACTLY today's connect path because it
    // literally is that call.
    let _sb = EnvSandbox::enter("plain", false);
    let src = local_source_repo("plain-src"); // planning-less

    // Clone leg first, while `src` is still pristine.
    let dest =
        perform_clone(src.to_str().unwrap(), "sw-plain").expect("offline local-source clone lands");
    assert!(gitops::is_work_tree(&dest));

    // Identity clause under the enforced model: the SAME planning-less
    // tree, reached once through the clone and once by hand-typing the
    // source's path, meets the same planning-setup gate — neither leg
    // may silently adopt an unregistered checkout or write project state there.
    for (path, what) in [(dest.as_path(), "clone leg"), (src.as_path(), "typed leg")] {
        match attempt_connect(path.to_str().unwrap()) {
            Ok(project) => panic!(
                "{what}: planning-less checkout must not connect ({:?})",
                project.state.title
            ),
            Err(err) => {
                let msg = format!("{} | {}", err.headline(), err.detail());
                assert!(
                    msg.contains("planning repository"),
                    "{what}: actionable planning-setup guidance (got: {msg})"
                );
            }
        }
        assert!(
            !path.join(".koolade-packet").exists(),
            "{what}: the refusal must not plant any state"
        );
    }
    let _ = std::fs::remove_dir_all(&src);
}

#[test]
fn perform_clone_refuses_a_preexisting_non_repository_directory() {
    let home = scratch_home("skip");
    let src = local_source_repo("skip");
    let dest = home.join("kool-ade-workspaces/sw-skip");
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(dest.join("marker.bin"), b"frozen-bytes").unwrap();

    match perform_clone_at(src.to_str().unwrap(), "sw-skip", &home) {
        Err(AppError::InvalidRepo { detail, .. }) => {
            assert!(
                detail.contains("will not adopt an existing checkout"),
                "{detail}"
            );
        }
        other => panic!("expected existing directory refusal, got {other:?}"),
    }
    assert_eq!(
        std::fs::read(dest.join("marker.bin")).unwrap(),
        b"frozen-bytes"
    );
    no_scratch_left(&home);
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&src);
}

#[cfg(unix)]
#[test]
fn perform_clone_refuses_a_symlink_at_the_managed_destination() {
    let home = scratch_home("symlink");
    let src = local_source_repo("symlink");
    let parent = home.join("kool-ade-workspaces");
    std::fs::create_dir_all(&parent).unwrap();
    let target = home.join("operator-checkout");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("marker.bin"), b"keep me").unwrap();
    let dest = parent.join("sw-symlink");
    std::os::unix::fs::symlink(&target, &dest).unwrap();

    match perform_clone_at(src.to_str().unwrap(), "sw-symlink", &home) {
        Err(AppError::InvalidRepo { detail, .. }) => {
            assert!(detail.contains("symbolic link"), "{detail}");
        }
        other => panic!("expected symlink refusal, got {other:?}"),
    }
    assert!(
        std::fs::symlink_metadata(&dest)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        std::fs::read(target.join("marker.bin")).unwrap(),
        b"keep me"
    );
    no_scratch_left(&home);
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&src);
}

#[test]
fn perform_clone_preserves_an_unowned_staging_directory() {
    let home = scratch_home("sweep");
    let src = local_source_repo("sweep");
    let stale = home.join("kool-ade-workspaces/sw-sweep.koolade-cloning.999");
    std::fs::create_dir_all(stale.join("junk")).unwrap();
    std::fs::write(stale.join("junk/leftover.txt"), "dead").unwrap();

    let dest = perform_clone_at(src.to_str().unwrap(), "sw-sweep", &home).expect("clone lands");
    assert!(stale.is_dir(), "unowned staging directories are preserved");
    assert_eq!(
        std::fs::read(stale.join("junk/leftover.txt")).unwrap(),
        b"dead"
    );
    assert!(gitops::is_work_tree(&dest));
    let staging = std::fs::read_dir(home.join("kool-ade-workspaces"))
        .unwrap()
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("sw-sweep.koolade-cloning.")
        })
        .filter(|entry| entry.path() != stale)
        .collect::<Vec<_>>();
    assert!(
        staging.is_empty(),
        "this clone left no staging folder: {staging:?}"
    );
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
