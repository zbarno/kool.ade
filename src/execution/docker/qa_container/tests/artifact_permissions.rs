use super::*;

#[test]
fn artifact_snapshot_directories_are_writable_during_extraction() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("dist")).unwrap();
    std::fs::write(root.path().join("dist/index.html"), b"approved build").unwrap();
    let mount = QaArtifactMount::inspect(root.path(), "/workspace").unwrap();
    let (upload, _) = mount.snapshot_upload(1024, None).unwrap();
    let mut archive = tar::Archive::new(upload.archive.as_slice());
    let directory = archive
        .entries()
        .unwrap()
        .find_map(|entry| {
            let entry = entry.unwrap();
            entry
                .header()
                .entry_type()
                .is_dir()
                .then(|| entry.header().mode().unwrap())
        })
        .unwrap();

    assert_eq!(directory & 0o777, 0o755);
}
