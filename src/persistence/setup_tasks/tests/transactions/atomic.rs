use std::fs;

use super::*;

/// Production boundary: corrupt, symlinked, and unreplaceable local files must
/// fail closed without changing their bytes. Model tests cannot exercise path
/// metadata, symlink handling, or atomic rename failure. Setup uses temporary
/// files only; symlink coverage is Unix-only, no external services, low cost.
#[test]
fn corrupt_symlinked_and_unreplaceable_store_paths_are_preserved() {
    let temp = tempfile::tempdir().unwrap();
    let parent = temp.path().join("machine-setup");
    fs::create_dir_all(&parent).unwrap();

    let corrupt = parent.join("corrupt.json");
    let corrupt_bytes = b"{ truncated";
    fs::write(&corrupt, corrupt_bytes).unwrap();
    assert!(
        transaction::update_at(&corrupt, |store| {
            store.latest_scan_generation += 1;
            Ok(((), true))
        })
        .is_err()
    );
    assert_eq!(fs::read(&corrupt).unwrap(), corrupt_bytes);

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let target = parent.join("target.json");
        let target_bytes = b"preserve target";
        fs::write(&target, target_bytes).unwrap();
        let link = parent.join("linked.json");
        symlink(&target, &link).unwrap();
        assert!(transaction::update_at(&link, |store| Ok((store.clone(), true))).is_err());
        assert_eq!(fs::read(&target).unwrap(), target_bytes);
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    let blocked = parent.join("directory.json");
    fs::create_dir_all(&blocked).unwrap();
    let sentinel = blocked.join("sentinel");
    fs::write(&sentinel, b"keep").unwrap();
    assert!(transaction::save_to(&blocked, &SetupTaskStoreV1::default()).is_err());
    assert_eq!(fs::read(&sentinel).unwrap(), b"keep");
}
