use super::*;

fn compressed_release(index_text: &str) -> (SignedRelease, Vec<u8>, &'static str) {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(index_text.as_bytes()).unwrap();
    let bytes = encoder.finish().unwrap();
    let path = "main/binary-amd64/Packages.gz";
    let release = SignedRelease {
        origin: "Debian".into(),
        label: "Debian".into(),
        suite: "bookworm".into(),
        codename: "bookworm".into(),
        valid_until_unix: 0,
        architectures: vec!["amd64".into()],
        components: vec!["main".into()],
        sha256: BTreeMap::from([(
            path.into(),
            (format!("{:x}", Sha256::digest(&bytes)), bytes.len() as u64),
        )]),
    };
    (release, bytes, path)
}

#[test]
fn compressed_index_selects_the_newest_signed_version_for_duplicate_package_names() {
    let index_text = concat!(
        "Package: linux-doc\nVersion: 6.1.170-3\nArchitecture: all\nFilename: pool/main/l/linux/linux-doc_6.1.170-3_all.deb\nSize: 12\nSHA256: ",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n\n",
        "Package: linux-doc\nVersion: 6.1.176-1\nArchitecture: all\nFilename: pool/main/l/linux/linux-doc_6.1.176-1_all.deb\nSize: 12\nSHA256: ",
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n\n"
    );
    let (release, bytes, path) = compressed_release(index_text);

    let parsed = PackageIndex::parse_gzip(&release, path, &bytes).unwrap();
    let closure = parsed
        .closure(&["linux-doc".into()], &BTreeMap::new(), "amd64")
        .unwrap();
    assert_eq!(closure.len(), 1);
    assert_eq!(closure[0].version, "6.1.176-1");
    assert_eq!(
        closure[0].sha256,
        "b".repeat(64),
        "the selected artifact identity must come from the highest Debian version"
    );
}

#[test]
fn compressed_index_rejects_duplicate_package_versions_as_ambiguous() {
    let index_text = concat!(
        "Package: sample\nVersion: 1.0-1\nArchitecture: amd64\nFilename: pool/main/s/sample_1.0-1_amd64.deb\nSize: 12\nSHA256: ",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n\n",
        "Package: sample\nVersion: 1.0-1\nArchitecture: amd64\nFilename: pool/main/s/other_1.0-1_amd64.deb\nSize: 12\nSHA256: ",
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n\n"
    );
    let (release, bytes, path) = compressed_release(index_text);
    assert!(
        PackageIndex::parse_gzip(&release, path, &bytes)
            .unwrap_err()
            .to_string()
            .contains("duplicate package name and version")
    );
}

#[test]
fn compressed_index_rejects_duplicate_package_names_across_architectures() {
    let index_text = concat!(
        "Package: sample\nVersion: 1.0-1\nArchitecture: amd64\nFilename: pool/main/s/sample_1.0-1_amd64.deb\nSize: 12\nSHA256: ",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n\n",
        "Package: sample\nVersion: 1.1-1\nArchitecture: arm64\nFilename: pool/main/s/sample_1.1-1_arm64.deb\nSize: 12\nSHA256: ",
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n\n"
    );
    let (release, bytes, path) = compressed_release(index_text);
    let error = PackageIndex::parse_gzip(&release, path, &bytes).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("duplicate package names across architectures"),
        "unexpected parser error: {error:#}"
    );
}
