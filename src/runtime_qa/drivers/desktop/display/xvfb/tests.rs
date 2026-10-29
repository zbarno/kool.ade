use std::path::Path;

use super::require;

#[test]
fn missing_xvfb_is_reported_as_an_unavailable_prerequisite() {
    let directory = tempfile::tempdir().unwrap();
    let missing_xvfb = directory.path().join("Xvfb");

    assert_eq!(
        require(Path::new(&missing_xvfb)).unwrap_err(),
        "pinned QA image does not provide Xvfb"
    );
}
