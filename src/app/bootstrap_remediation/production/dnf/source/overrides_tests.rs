use super::reject_candidate_repository_sections;

#[test]
fn unrelated_repository_overrides_do_not_change_the_candidate_set() {
    reject_candidate_repository_sections(
        b"[third-party]\nenabled=0\n[updates-testing]\nenabled=0\n",
    )
    .unwrap();
}

#[test]
fn candidate_and_matching_wildcard_overrides_are_refused() {
    for contents in [
        b"[fedora]\nbaseurl=https://example.invalid/\n".as_slice(),
        b"[updates*]\nenabled=0\n".as_slice(),
        b"[koolade-*]\ngpgkey=https://example.invalid/key\n".as_slice(),
        b"[*]\nbaseurl=https://example.invalid/\n".as_slice(),
    ] {
        assert!(reject_candidate_repository_sections(contents).is_err());
    }
}

#[test]
fn duplicate_candidate_override_ids_are_refused() {
    assert!(
        reject_candidate_repository_sections(
            b"[fedora]\nenabled=1\n[fedora]\ngpgkey=https://example.invalid/key\n"
        )
        .is_err()
    );
}
