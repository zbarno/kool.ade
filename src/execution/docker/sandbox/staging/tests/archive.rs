use super::*;

#[cfg(unix)]
#[test]
fn returned_archive_excludes_nested_git_metadata() {
    let archive = returned(&[("nested/.git/config", b"untrusted metadata")]);
    // Tar archives are padded to 10 KiB blocks, even when their payload is
    // tiny. Keep this above the archive size so the test reaches path filtering.
    let entries = super::super::archive::parse_returned(&archive, 16 * 1024, 10).unwrap();
    assert!(entries.is_empty());
}

#[cfg(unix)]
#[test]
fn returned_archive_accepts_tar_dot_slash_prefixes() {
    let archive = returned(&[("./output.txt", b"container output")]);
    let entries = super::super::archive::parse_returned(&archive, 16 * 1024, 10).unwrap();
    assert_eq!(
        entries.get(Path::new("output.txt")).unwrap().bytes,
        b"container output"
    );
}

#[cfg(unix)]
#[test]
fn returned_archive_materializes_bounded_hard_links_from_prior_files() {
    let archive = returned_with_hard_link(
        "./target/debug/deps/fixture",
        "./target/debug/fixture",
        "./target/debug/fixture",
    );
    let entries = super::super::archive::parse_returned(&archive, 16 * 1024, 10).unwrap();
    assert_eq!(
        entries
            .get(Path::new("target/debug/deps/fixture"))
            .unwrap()
            .bytes,
        b"payload"
    );
    let fixture = Fixture::new();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    let snapshot = capture(&workspace, 1024 * 1024, false).unwrap();
    synchronize(&workspace, &snapshot, &archive, 1024 * 1024).unwrap();
    assert_eq!(
        fs::read(fixture.0.join("target/debug/deps/fixture")).unwrap(),
        b"payload"
    );
}

#[cfg(unix)]
#[test]
fn returned_archive_rejects_hard_links_that_escape_the_workspace() {
    let archive = returned_with_hard_link("inside", "source", "../outside");
    assert!(super::super::archive::parse_returned(&archive, 16 * 1024, 10).is_err());
}
