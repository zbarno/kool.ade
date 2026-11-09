use super::*;

#[test]
fn archive_validation_rejects_tar_links_and_excessive_expansion() {
    let mut archive = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Symlink);
    header.set_size(0);
    header.set_mode(0o777);
    header.set_cksum();
    archive
        .append_link(&mut header, "link.txt", Path::new("../../secret"))
        .unwrap();
    let bytes = archive.into_inner().unwrap();
    assert!(super::super::validation::validate_content("unsafe.tar", &bytes).is_err());
    let mut compressed = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut compressed, &bytes).unwrap();
    let compressed = compressed.finish().unwrap();
    assert!(super::super::validation::validate_content("unsafe.tar.gz", &compressed).is_err());
}

#[test]
fn normalized_paths_and_display_names_reject_ambiguous_or_unsafe_values() {
    for path in [
        "../secret.txt",
        "/etc/passwd",
        "C:/secret.txt",
        "out\\..\\secret",
        "NUL.txt",
        "out/evil\u{202e}txt",
    ] {
        assert!(
            !super::super::validation::valid_relative_path(path),
            "accepted unsafe path {path:?}"
        );
    }
    for name in [
        "../secret.txt",
        "NUL.txt",
        "image\u{2066}.png",
        "trailing.",
        "bad?.txt",
    ] {
        assert!(
            !super::super::validation::valid_display_name(name),
            "accepted unsafe name {name:?}"
        );
    }
}

#[test]
fn archive_validation_rejects_declared_tar_expansion_above_the_quota() {
    let mut header = tar::Header::new_gnu();
    header.set_path("large.txt").unwrap();
    header.set_entry_type(tar::EntryType::Regular);
    header.set_size(COMPUTER_ARTIFACT_MAX_BYTES_V1 + 1);
    header.set_mode(0o600);
    header.set_cksum();
    let mut bytes = header.as_bytes().to_vec();
    bytes.resize(1024, 0);
    assert!(super::super::validation::validate_content("large.tar", &bytes).is_err());
}
