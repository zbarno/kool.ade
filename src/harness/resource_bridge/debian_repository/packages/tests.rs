use super::index::ProvidedPackage;
use super::*;
use crate::harness::resource_bridge::debian_repository::SignedRelease;
use flate2::{Compression, write::GzEncoder};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Write};

fn record(name: &str, depends: Option<&str>) -> PackageRecord {
    let mut fields = BTreeMap::from([
        ("Package".into(), name.into()),
        ("Version".into(), "1.0-1".into()),
        ("Architecture".into(), "amd64".into()),
        (
            "Filename".into(),
            format!("pool/main/{}/{}_1.0-1_amd64.deb", &name[..1], name),
        ),
        ("Size".into(), "12".into()),
        ("SHA256".into(), "a".repeat(64)),
    ]);
    if let Some(depends) = depends {
        fields.insert("Depends".into(), depends.into());
    }
    parse_record(&fields).unwrap()
}

#[test]
fn dependency_closure_is_transitive_arch_checked_and_sorted() {
    let index = PackageIndex {
        records: BTreeMap::from([
            ("app".into(), record("app", Some("liba"))),
            ("liba".into(), record("liba", Some("libb"))),
            ("libb".into(), record("libb", None)),
        ]),
    };
    let closure = index
        .closure(&["app".into()], &BTreeMap::new(), "amd64")
        .unwrap();
    assert_eq!(
        closure
            .iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>(),
        ["app", "liba", "libb"]
    );
    assert!(
        index
            .closure(&["app".into()], &BTreeMap::new(), "arm64")
            .is_err()
    );
}

#[test]
fn closure_uses_versioned_virtual_providers_and_base_versions() {
    let mut app = record("app", Some("virtual-abi (>= 2.0) | fallback"));
    let mut provider = record("provider", None);
    provider.provides.push(ProvidedPackage {
        name: "virtual-abi".into(),
        version: Some("2.1".into()),
    });
    let index = PackageIndex {
        records: BTreeMap::from([
            ("app".into(), app.clone()),
            ("provider".into(), provider),
            ("fallback".into(), record("fallback", None)),
        ]),
    };
    let closure = index
        .closure(&["app".into()], &BTreeMap::new(), "amd64")
        .unwrap();
    assert_eq!(
        closure
            .iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>(),
        ["app", "provider"]
    );

    app.dependencies = super::dependencies::parse_dependencies("liba (>= 1.0)").unwrap();
    let index = PackageIndex {
        records: BTreeMap::from([("app".into(), app), ("liba".into(), record("liba", None))]),
    };
    let base = BTreeMap::from([(
        "liba".into(),
        super::InstalledPackage {
            version: "2.0".into(),
            architecture: "amd64".into(),
            multi_arch: None,
        },
    )]);
    let closure = index.closure(&["app".into()], &base, "amd64").unwrap();
    assert_eq!(
        closure
            .iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>(),
        ["app"]
    );
    let stale = BTreeMap::from([(
        "liba".into(),
        super::InstalledPackage {
            version: "0.9".into(),
            architecture: "amd64".into(),
            multi_arch: None,
        },
    )]);
    let closure = index.closure(&["app".into()], &stale, "amd64").unwrap();
    assert_eq!(
        closure
            .iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>(),
        ["app", "liba"]
    );
}

#[test]
fn malformed_or_unsupported_dependency_semantics_fail_closed() {
    for expr in ["liba (>= 2", "liba <!nocheck>", "liba:foreign"] {
        assert!(
            parse_record(&BTreeMap::from([
                ("Package".into(), "app".into()),
                ("Version".into(), "1".into()),
                ("Architecture".into(), "amd64".into()),
                ("Filename".into(), "pool/main/a/app.deb".into()),
                ("Size".into(), "1".into()),
                ("SHA256".into(), "a".repeat(64)),
                ("Depends".into(), expr.into()),
            ]))
            .is_err()
        );
    }
    assert!(
        parse_record(&BTreeMap::from([
            ("Package".into(), "app".into()),
            ("Version".into(), "1".into()),
            ("Architecture".into(), "amd64".into()),
            ("Filename".into(), "pool/main/../../evil.deb".into()),
            ("Size".into(), "1".into()),
            ("SHA256".into(), "a".repeat(64)),
        ]))
        .is_err()
    );
}

#[test]
fn any_qualified_dependencies_require_multi_arch_allowed() {
    let app = record("app", Some("liba:any"));
    let index = PackageIndex {
        records: BTreeMap::from([("app".into(), app), ("liba".into(), record("liba", None))]),
    };
    assert!(
        index
            .closure(&["app".into()], &BTreeMap::new(), "amd64")
            .is_err()
    );
    let mut fields = BTreeMap::from([
        ("Package".into(), "liba".into()),
        ("Version".into(), "1.0-1".into()),
        ("Architecture".into(), "amd64".into()),
        ("Multi-Arch".into(), "allowed".into()),
        ("Filename".into(), "pool/main/l/liba_1.0-1_amd64.deb".into()),
        ("Size".into(), "12".into()),
        ("SHA256".into(), "a".repeat(64)),
    ]);
    let mut records = index.records;
    records.insert("liba".into(), parse_record(&fields).unwrap());
    fields.clear();
    let index = PackageIndex { records };
    assert_eq!(
        index
            .closure(&["app".into()], &BTreeMap::new(), "amd64")
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn package_bytes_must_match_signed_index_size_and_digest() {
    let bytes = b"synthetic-deb".to_vec();
    let record = PackageRecord {
        name: "sample".into(),
        version: "1.0-1".into(),
        architecture: "amd64".into(),
        multi_arch: None,
        filename: "pool/main/s/sample_1.0-1_amd64.deb".into(),
        size: bytes.len() as u64,
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        dependencies: Vec::new(),
        provides: Vec::<ProvidedPackage>::new(),
    };
    let verified = record.verify_artifact(bytes.clone()).unwrap();
    assert_eq!(verified.bytes, bytes);
    assert!(record.verify_artifact(b"tampered".to_vec()).is_err());
}

#[test]
fn compressed_index_is_hash_checked_before_records_are_admitted() {
    let index_text = concat!(
        "Package: app\nVersion: 1.0-1\nArchitecture: amd64\nFilename: pool/main/a/app_1.0-1_amd64.deb\nSize: 12\nSHA256: ",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\nDepends: liba\n\n",
        "Package: liba\nVersion: 2.0-1\nArchitecture: all\nFilename: pool/main/libl/liba_2.0-1_all.deb\nSize: 12\nSHA256: ",
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n\n"
    );
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
    let parsed = PackageIndex::parse_gzip(&release, path, &bytes).unwrap();
    let closure = parsed
        .closure(&["app".into()], &BTreeMap::new(), "amd64")
        .unwrap();
    assert_eq!(
        closure
            .iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>(),
        ["app", "liba"]
    );
    let mut tampered = bytes;
    tampered.push(0);
    assert!(PackageIndex::parse_gzip(&release, path, &tampered).is_err());
}
