use super::*;

#[test]
// Production boundary: stage-out pins the opened workspace root and every
// traversed output directory against rename for the duration of the apply.
// A unit check cannot prove Windows share-delete semantics. Uses only a local
// temp directory; no Docker, network, or special setup is required.
fn stage_out_pins_root_and_output_parent_until_apply_finishes() {
    let fixture = tempfile::tempdir().unwrap();
    let nested = fixture.path().join("nested");
    fs::create_dir(&nested).unwrap();
    let moved_nested = fixture.path().join("moved-nested");
    let moved_root = fixture.path().with_extension("moved");
    let update = returned("nested/new.txt", b"container");

    apply_test(
        fixture.path(),
        &Manifest::new(),
        &update,
        None,
        |checkpoint| {
            if matches!(checkpoint, Checkpoint::BeforeCreateInstall { .. }) {
                assert!(fs::rename(&nested, &moved_nested).is_err());
                assert!(fs::rename(fixture.path(), &moved_root).is_err());
            }
            false
        },
    )
    .unwrap();

    assert_eq!(
        fs::read(fixture.path().join("nested/new.txt")).unwrap(),
        b"container"
    );
    assert!(!moved_nested.exists());
    assert!(!moved_root.exists());
}
