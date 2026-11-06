use std::io::Cursor;

use crate::computer_use::guest_image::create_guest_image_overlay;

use super::{AllowExact, GuestImageCacheV1, verified};

#[cfg(unix)]
#[test]
fn overlay_factory_uses_verified_read_only_base_and_allocates_independent_files() {
    use std::{fs, os::unix::fs::PermissionsExt};

    // Production boundary: the host VM backend passes an immutable cache image
    // into qemu-img. A unit validator cannot prove process argv/backing-file
    // behavior. A deterministic fake qemu-img is used (no KVM/network/SSH); low setup.
    let artifact = b"synthetic qcow2 bytes";
    let verified = verified(artifact);
    let temp = tempfile::tempdir().unwrap();
    let cache = GuestImageCacheV1::new(temp.path().join("cache")).unwrap();
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
    let fake = temp.path().join("qemu-img");
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
    let overlay_root = temp.path().join("overlays");
    let first = create_guest_image_overlay(
        &cached,
        "00000000-0000-4000-8000-000000000001",
        &overlay_root,
        &fake,
    )
    .unwrap();
    let second = create_guest_image_overlay(
        &cached,
        "00000000-0000-4000-8000-000000000002",
        &overlay_root,
        &fake,
    )
    .unwrap();
    assert_ne!(first.path(), second.path());
    assert!(first.path().is_file() && second.path().is_file());
    assert_eq!(fs::read(cached.path()).unwrap(), artifact);
    assert_eq!(
        fs::metadata(cached.path()).unwrap().permissions().mode() & 0o222,
        0
    );
    assert_eq!(
        fs::metadata(first.path()).unwrap().permissions().mode() & 0o777,
        0o600
    );
}
