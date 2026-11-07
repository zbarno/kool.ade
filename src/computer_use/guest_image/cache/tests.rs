use super::{GuestImageCacheV1, ImageCacheErrorV1};

#[cfg(unix)]
#[test]
fn opening_existing_cache_is_read_only_and_requires_private_directory() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    let missing = temp.path().join("missing-cache");
    assert_eq!(
        GuestImageCacheV1::open_existing(missing.clone()).unwrap_err(),
        ImageCacheErrorV1::CachePathUnsafe
    );
    assert!(
        !missing.exists(),
        "read-only open must not create the cache"
    );

    let cache_root = temp.path().join("cache");
    std::fs::create_dir(&cache_root).unwrap();
    std::fs::set_permissions(&cache_root, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        GuestImageCacheV1::open_existing(cache_root.clone()).unwrap_err(),
        ImageCacheErrorV1::CachePathUnsafe
    );
    assert_eq!(
        std::fs::symlink_metadata(&cache_root)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755,
        "failed read-only open must not repair permissions"
    );

    GuestImageCacheV1::new(cache_root.clone()).unwrap();
    GuestImageCacheV1::open_existing(cache_root).unwrap();

    let target = temp.path().join("real-cache");
    std::fs::create_dir(&target).unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o700)).unwrap();
    let alias = temp.path().join("cache-alias");
    std::os::unix::fs::symlink(&target, &alias).unwrap();
    assert_eq!(
        GuestImageCacheV1::open_existing(alias).unwrap_err(),
        ImageCacheErrorV1::CachePathUnsafe
    );
}
