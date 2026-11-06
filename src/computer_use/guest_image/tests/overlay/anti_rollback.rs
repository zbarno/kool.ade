use super::super::super::{
    ComputerGuestArchitectureV1, GuestImageCacheV1, GuestImageCatalogGenerationFloorV1,
};
use super::{CONTROLLER_KEY_ID, ImageCacheErrorV1, TEST_NOW, fixture_generation};

#[test]
fn launch_persists_generation_and_rejects_signed_catalog_replay_after_reopen() {
    use std::io::Cursor;

    // Production boundary: durable floor survives cache/store object recreation
    // and blocks replay before any overlay is created. Uses signed synthetic
    // catalogs and fake QEMU; no VM, Docker, network, or SSH; low setup cost.
    let artifact = b"synthetic qcow2 bytes";
    let generation_one = fixture_generation(artifact, false, 1);
    let cached = super::super::verified(artifact);
    let temp = tempfile::tempdir().unwrap();
    let cache = GuestImageCacheV1::new(temp.path().join("cache")).unwrap();
    cache
        .install_approved_reader(
            &cached,
            &super::super::AllowExact {
                manifest: cached.manifest_digest().into(),
                image: cached.entry().image_digest.clone(),
            },
            &mut Cursor::new(artifact),
        )
        .unwrap();
    let root = temp.path().join("app-state/catalog-trust");
    std::fs::create_dir_all(root.parent().unwrap()).unwrap();
    let floor =
        GuestImageCatalogGenerationFloorV1::initialize_for_first_install(root.clone(), 1).unwrap();
    let generation_two = fixture_generation(artifact, false, 2);
    let qemu = super::fake_qemu(temp.path());
    let overlay_root = temp.path().join("overlays");
    let overlay =
        super::super::super::super::guest_image::overlay::create_guest_image_overlay_at_for_test(
            &cache,
            &serde_json::to_vec(&generation_two.manifest).unwrap(),
            &generation_two.trust,
            &generation_two.session,
            TEST_NOW,
            ComputerGuestArchitectureV1::X86_64,
            CONTROLLER_KEY_ID,
            &generation_two.controller.verifying_key(),
            &floor,
            &overlay_root,
            &qemu,
        )
        .unwrap();
    overlay.release_after_vm_stopped().unwrap();
    drop(floor);

    let reopened = GuestImageCatalogGenerationFloorV1::open(root).unwrap();
    let replay =
        super::super::super::super::guest_image::overlay::create_guest_image_overlay_at_for_test(
            &cache,
            &serde_json::to_vec(&generation_one.manifest).unwrap(),
            &generation_one.trust,
            &generation_one.session,
            TEST_NOW,
            ComputerGuestArchitectureV1::X86_64,
            CONTROLLER_KEY_ID,
            &generation_one.controller.verifying_key(),
            &reopened,
            &overlay_root,
            &qemu,
        );
    assert!(matches!(
        replay,
        Err(super::super::super::super::guest_image::GuestImageOverlayErrorV1::CatalogRollback)
    ));
    assert_eq!(std::fs::read_dir(&overlay_root).unwrap().count(), 0);
    assert_eq!(
        super::super::super::super::guest_image::overlay::create_guest_image_overlay_at_for_test(
            &cache,
            &serde_json::to_vec(&generation_two.manifest).unwrap(),
            &generation_two.trust,
            &generation_two.session,
            TEST_NOW,
            ComputerGuestArchitectureV1::X86_64,
            CONTROLLER_KEY_ID,
            &generation_two.controller.verifying_key(),
            &reopened,
            &overlay_root,
            &qemu,
        )
        .map(|overlay| overlay.release_after_vm_stopped().unwrap()),
        Ok(())
    );
    assert_eq!(
        GuestImageCatalogGenerationFloorV1::open(temp.path().join("missing"))
            .err()
            .unwrap(),
        ImageCacheErrorV1::CatalogFloorUnavailable
    );
}
