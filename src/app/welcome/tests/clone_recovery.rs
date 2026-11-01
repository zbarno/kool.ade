use super::helpers::{local_source_repo, scratch_home};
use crate::{
    app::welcome::github::{
        clear_pending_clone_receipt, pending_clone_workspace_id_at, perform_clone_at_for_workspace,
    },
    error::AppError,
};

#[test]
// Production boundary: the managed clone adapter records its workspace before
// publishing the checkout, then resumes only that repository/workspace pair.
// This exercises a filesystem clone and retry across calls; a receipt parser
// unit test cannot prove the checkout is safely reused after publication.
// External services: local Git only. Setup cost: one temporary source repo and
// one temporary clone.
fn interrupted_managed_clone_resumes_only_for_its_saved_workspace() {
    let home = scratch_home("recovery");
    let source = local_source_repo("recovery");
    let source = source.to_string_lossy().into_owned();
    let workspace_id = uuid::Uuid::new_v4();
    let destination = perform_clone_at_for_workspace(&source, "sw-recovery", &home, workspace_id)
        .expect("first managed clone completes");

    assert_eq!(
        pending_clone_workspace_id_at(&source, "sw-recovery", &home).unwrap(),
        Some(workspace_id)
    );
    assert!(matches!(
        perform_clone_at_for_workspace(&source, "sw-recovery", &home, uuid::Uuid::new_v4()),
        Err(AppError::InvalidRepo { .. })
    ));
    assert_eq!(
        perform_clone_at_for_workspace(&source, "sw-recovery", &home, workspace_id).unwrap(),
        destination
    );

    clear_pending_clone_receipt(&destination, &source, workspace_id).unwrap();
    assert_eq!(
        pending_clone_workspace_id_at(&source, "sw-recovery", &home).unwrap(),
        None
    );
    assert!(matches!(
        crate::app::welcome::perform_clone_at(&source, "sw-recovery", &home),
        Err(AppError::InvalidRepo { .. })
    ));

    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(source);
}

#[test]
// Production boundary: clone retry must distinguish its own staged Git clone
// from a pre-existing checkout with the same origin. A receipt-parser test
// cannot prove this because authorization depends on Git metadata and the
// managed destination filesystem. External services: local Git only. Setup
// cost: one temporary source repo and one temporary clone.
fn repeated_setup_never_adopts_a_preexisting_same_origin_checkout() {
    let home = scratch_home("preexisting");
    let source = local_source_repo("preexisting");
    let source_text = source.to_string_lossy().into_owned();
    let destination = crate::app::welcome::perform_clone_at(&source_text, "sw-existing", &home)
        .expect("seed an unrelated same-origin checkout");
    std::fs::write(destination.join("operator-notes.txt"), "keep this file\n").unwrap();
    let workspace_id = uuid::Uuid::new_v4();

    for attempt in 0..2 {
        let result =
            perform_clone_at_for_workspace(&source_text, "sw-existing", &home, workspace_id);
        assert!(
            matches!(result, Err(AppError::InvalidRepo { .. })),
            "attempt {attempt} adopted an existing checkout: {result:?}"
        );
    }
    assert_eq!(
        std::fs::read_to_string(destination.join("operator-notes.txt")).unwrap(),
        "keep this file\n"
    );
    assert_eq!(
        pending_clone_workspace_id_at(&source_text, "sw-existing", &home).unwrap(),
        None
    );
    let parent = destination.parent().unwrap();
    assert!(
        !parent
            .join(".sw-existing.koolade-pending-clone.json")
            .exists()
    );
    assert!(
        !parent
            .join(".sw-existing.koolade-pending-setup.json")
            .exists()
    );

    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(source);
}

#[cfg(unix)]
#[test]
// Production boundary: retry validates the exact managed checkout before
// reopening an interrupted clone. Unit checks cannot prove symlink handling
// through the real Git checkout and sidecar receipt. External services: local
// Git only. Setup cost: one temporary source repo and one temporary clone.
fn interrupted_clone_receipt_does_not_authorize_a_symlink_destination() {
    let home = scratch_home("symlink-recovery");
    let source = local_source_repo("symlink-recovery");
    let source_text = source.to_string_lossy().into_owned();
    let workspace_id = uuid::Uuid::new_v4();
    let destination =
        perform_clone_at_for_workspace(&source_text, "sw-symlink-recovery", &home, workspace_id)
            .expect("managed clone has a recovery receipt");
    let parent = destination.parent().unwrap();
    let preserved_clone = parent.join("preserved-clone");
    std::fs::rename(&destination, &preserved_clone).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&source, &destination).unwrap();

    assert!(pending_clone_workspace_id_at(&source_text, "sw-symlink-recovery", &home).is_err());
    assert!(matches!(
        perform_clone_at_for_workspace(&source_text, "sw-symlink-recovery", &home, workspace_id,),
        Err(AppError::InvalidRepo { .. })
    ));
    assert!(
        preserved_clone
            .join(".git/koolade-pending-clone.json")
            .is_file()
    );

    std::fs::remove_file(destination).unwrap();
    let _ = std::fs::remove_dir_all(preserved_clone);
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(source);
}
