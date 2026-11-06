use ed25519_dalek::{Signer, SigningKey};
use std::{io::Cursor, path::Path};

use crate::computer_use::guest_image::{
    ComputerGuestArchitectureV1, GuestImageManifestTrustV1, GuestImageOverlayRequestV1,
    GuestImageStatusV1, create_guest_image_overlay,
};

use super::super::super::{ComputerSessionV1, guest_image::process::QemuImageToolV1};
use super::super::GuestImageCatalogGenerationFloorV1;

#[path = "overlay/anti_rollback.rs"]
mod anti_rollback;
use super::{
    CATALOG_KEY_ID, CONTROLLER_KEY_ID, ComputerImageDownloadApprovalV1, GuestImageCacheV1,
    GuestImageManifestV1, ImageCacheErrorV1, encode_hex, signed_catalog,
};

const TEST_NOW: u64 = 1_050_000;

struct Fixture {
    manifest: GuestImageManifestV1,
    trust: GuestImageManifestTrustV1,
    session: ComputerSessionV1,
    controller: SigningKey,
}

fn fixture(artifact: &[u8], revoked: bool) -> Fixture {
    fixture_generation(artifact, revoked, 1)
}

fn fixture_generation(artifact: &[u8], revoked: bool, generation: u64) -> Fixture {
    let (mut manifest, catalog_key) = signed_catalog(artifact);
    manifest.catalog_generation = generation;
    if revoked {
        manifest.images[0].status = GuestImageStatusV1::Revoked;
    }
    manifest.signature_hex = encode_hex(
        &SigningKey::from_bytes(&[33; 32])
            .sign(&manifest.signing_payload_bytes().unwrap())
            .to_bytes(),
    );
    let mut session: ComputerSessionV1 = serde_json::from_str(
        &serde_json::from_str::<serde_json::Value>(include_str!(
            "../../../../tests/fixtures/computer_use/contracts-v1.json"
        ))
        .unwrap()["session"]
            .to_string(),
    )
    .unwrap();
    session.image_digest = manifest.images[0].image_digest.clone();
    session.image_manifest_digest = manifest.digest().unwrap();
    session.image_signer_key_id = CATALOG_KEY_ID.into();
    let controller = SigningKey::from_bytes(&[42; 32]);
    session
        .attest(CONTROLLER_KEY_ID.into(), &controller)
        .unwrap();
    let trust = GuestImageManifestTrustV1::from_application_pinned_keys(
        [(CATALOG_KEY_ID.into(), catalog_key.verifying_key())],
        1,
    )
    .unwrap();
    Fixture {
        manifest,
        trust,
        session,
        controller,
    }
}

struct AllowExact {
    manifest: String,
    image: String,
}

impl ComputerImageDownloadApprovalV1 for AllowExact {
    fn authorize_download(
        &self,
        manifest_digest: &str,
        image_digest: &str,
    ) -> Result<(), ImageCacheErrorV1> {
        if manifest_digest == self.manifest && image_digest == self.image {
            Ok(())
        } else {
            Err(ImageCacheErrorV1::ApprovalDenied)
        }
    }
}

fn fake_qemu(root: &Path) -> QemuImageToolV1 {
    use std::{fs, os::unix::fs::PermissionsExt};
    let fake = root.join("qemu-img");
    fs::write(
        &fake,
        r##"#!/bin/sh
if [ "$1" = create ]; then
  previous=; backing=; out=
  for arg do
    if [ "$previous" = -b ]; then backing=$arg; fi
    previous=$arg; out=$arg
  done
  printf 'synthetic overlay' > "$out"; printf '%s' "$backing" > "$out.backing"; exit 0
fi
if [ "$1" = info ]; then
  printf '{"format":"qcow2","virtual-size":2147483648,"backing-filename-format":"qcow2","backing-filename":"%s"}' "$(cat "$3.backing")"
  exit 0
fi
exit 2
"##,
    )
    .unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o700)).unwrap();
    QemuImageToolV1::from_test_path(fake)
}

#[cfg(unix)]
#[test]
fn launch_reverifies_fresh_manifest_and_creates_independent_overlays() {
    use std::{fs, os::unix::fs::PermissionsExt};

    // Production boundary: each overlay launch revalidates current catalog and
    // session authority before handing a cache path to QEMU. The fake executable
    // checks argv/backing behavior; no VM, Docker, network, or SSH; low setup.
    let artifact = b"synthetic qcow2 bytes";
    let fixture = fixture(artifact, false);
    let verified = super::verified(artifact);
    let temp = tempfile::tempdir().unwrap();
    let cache = GuestImageCacheV1::new(temp.path().join("cache")).unwrap();
    let floor = GuestImageCatalogGenerationFloorV1::initialize_for_first_install(
        temp.path().join("app-state"),
        1,
    )
    .unwrap();
    let cached = cache
        .install_approved_reader(
            &verified,
            &AllowExact {
                manifest: verified.manifest_digest().into(),
                image: verified.entry().image_digest.clone(),
            },
            &mut Cursor::new(artifact),
        )
        .unwrap();
    let qemu = fake_qemu(temp.path());
    let overlay_root = temp.path().join("overlays");
    let manifest_bytes = serde_json::to_vec(&fixture.manifest).unwrap();
    let first = super::super::super::guest_image::overlay::create_guest_image_overlay_at_for_test(
        &cache,
        &manifest_bytes,
        &fixture.trust,
        &fixture.session,
        TEST_NOW,
        ComputerGuestArchitectureV1::X86_64,
        CONTROLLER_KEY_ID,
        &fixture.controller.verifying_key(),
        &floor,
        &overlay_root,
        &qemu,
    )
    .unwrap();
    let second = super::super::super::guest_image::overlay::create_guest_image_overlay_at_for_test(
        &cache,
        &manifest_bytes,
        &fixture.trust,
        &fixture.session,
        TEST_NOW,
        ComputerGuestArchitectureV1::X86_64,
        CONTROLLER_KEY_ID,
        &fixture.controller.verifying_key(),
        &floor,
        &overlay_root,
        &qemu,
    )
    .unwrap();
    assert_eq!(
        cached.path(),
        cache.verify_cached(&verified).unwrap().path()
    );
    assert_ne!(first.path(), second.path());
    assert!(first.path().is_file() && second.path().is_file());
    assert_eq!(fs::read(cached.path()).unwrap(), artifact);
    super::super::super::guest_image::overlay::verify_overlay_before_boot_with_tool(&first, &qemu)
        .unwrap();
    super::super::super::guest_image::overlay::verify_overlay_before_boot_with_tool(&second, &qemu)
        .unwrap();
    fs::set_permissions(cached.path(), fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(cached.path(), b"changed image bytes").unwrap();
    assert_eq!(
        super::super::super::guest_image::overlay::verify_overlay_before_boot_with_tool(
            &first, &qemu,
        ),
        Err(crate::computer_use::guest_image::GuestImageOverlayErrorV1::BaseImageChanged)
    );
    assert_eq!(
        fs::metadata(first.path()).unwrap().permissions().mode() & 0o777,
        0o600
    );
    first.release_after_vm_stopped().unwrap();
    second.release_after_vm_stopped().unwrap();
}

#[cfg(unix)]
#[test]
fn launch_rejects_expired_or_revoked_fresh_catalog_before_qemu() {
    // Production boundary: stale cached approval cannot bypass current expiry
    // or revocation. Uses the exact overlay preparation path with a deterministic
    // clock; no VM, Docker, network, or SSH; low setup cost.
    let artifact = b"synthetic qcow2 bytes";
    let approved = super::verified(artifact);
    let temp = tempfile::tempdir().unwrap();
    let cache = GuestImageCacheV1::new(temp.path().join("cache")).unwrap();
    let floor = GuestImageCatalogGenerationFloorV1::initialize_for_first_install(
        temp.path().join("app-state"),
        1,
    )
    .unwrap();
    cache
        .install_approved_reader(
            &approved,
            &AllowExact {
                manifest: approved.manifest_digest().into(),
                image: approved.entry().image_digest.clone(),
            },
            &mut Cursor::new(artifact),
        )
        .unwrap();
    let qemu = fake_qemu(temp.path());
    let overlay_root = temp.path().join("overlays");

    let expired = fixture(artifact, false);
    let result = super::super::super::guest_image::overlay::create_guest_image_overlay_at_for_test(
        &cache,
        &serde_json::to_vec(&expired.manifest).unwrap(),
        &expired.trust,
        &expired.session,
        expired.manifest.expires_at_unix_ms + 1,
        ComputerGuestArchitectureV1::X86_64,
        CONTROLLER_KEY_ID,
        &expired.controller.verifying_key(),
        &floor,
        &overlay_root,
        &qemu,
    );
    assert!(matches!(
        result,
        Err(
            super::super::super::guest_image::GuestImageOverlayErrorV1::ManifestRejected(
                ImageCacheErrorV1::CatalogExpired
            )
        )
    ));

    let revoked = fixture(artifact, true);
    let result = super::super::super::guest_image::overlay::create_guest_image_overlay_at_for_test(
        &cache,
        &serde_json::to_vec(&revoked.manifest).unwrap(),
        &revoked.trust,
        &revoked.session,
        TEST_NOW,
        ComputerGuestArchitectureV1::X86_64,
        CONTROLLER_KEY_ID,
        &revoked.controller.verifying_key(),
        &floor,
        &overlay_root,
        &qemu,
    );
    assert!(matches!(
        result,
        Err(
            super::super::super::guest_image::GuestImageOverlayErrorV1::ManifestRejected(
                ImageCacheErrorV1::ImageRevoked
            )
        )
    ));
    assert!(!overlay_root.exists());
}

#[cfg(unix)]
#[test]
fn public_launch_api_does_not_accept_caller_selected_time_or_qemu_path() {
    // This compile-visible API test documents the public capability boundary:
    // qemu tool values can only come from the fixed host probe, and time is read
    // internally by create_guest_image_overlay; no external setup required.
    let _launch: fn(
        &GuestImageCacheV1,
        GuestImageOverlayRequestV1<'_>,
    ) -> Result<
        super::super::super::guest_image::GuestImageOverlayV1,
        super::super::super::guest_image::GuestImageOverlayErrorV1,
    > = create_guest_image_overlay;
}
