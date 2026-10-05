use super::lockfile::{
    LockedPackage, cache_covers_lockfile, collect_lockfiles, decode_base64, npm_cache_digest_path,
    verify_sha512_file,
};
use sha2::{Digest, Sha512};
use std::fs;

#[test]
fn reads_registry_entries_and_skips_local_workspace_entries() {
    let root = std::env::temp_dir().join(format!("koolade-npm-lock-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(root.join("packages/app")).unwrap();
    fs::write(
        root.join("package-lock.json"),
        r#"{"lockfileVersion":3,"packages":{"node_modules/pkg":{"resolved":"https://registry.npmjs.org/pkg/-/pkg-1.0.0.tgz","integrity":"sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=="},"packages/local":{"resolved":"file:../local"}}}"#,
    )
    .unwrap();
    fs::write(
        root.join("packages/app/package-lock.json"),
        r#"{"packages":{"node_modules/linked":{"resolved":"link:../linked"}}}"#,
    )
    .unwrap();
    let packages = collect_lockfiles(&root).unwrap();
    assert_eq!(
        packages,
        vec![LockedPackage {
            url: "https://registry.npmjs.org/pkg/-/pkg-1.0.0.tgz".into(),
            integrity: "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==".into(),
        }]
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn sha512_sri_maps_to_cacache_path_and_verifies_archive_bytes() {
    let directory = std::env::temp_dir().join(format!("koolade-npm-sri-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&directory).unwrap();
    let archive = directory.join("package.tgz");
    fs::write(&archive, b"locked package archive").unwrap();
    let digest = Sha512::digest(b"locked package archive");
    let encoded = base64(&digest);
    let integrity = format!("sha512-{encoded}");
    assert_eq!(decode_base64(&encoded).unwrap(), digest.as_slice());
    assert_eq!(
        npm_cache_digest_path(&directory, &integrity).unwrap(),
        directory
            .join("_cacache/content-v2/sha512")
            .join(hex(&digest[..1]))
            .join(hex(&digest[1..2]))
            .join(hex(&digest[2..]))
    );
    assert!(verify_sha512_file(&archive, &integrity));
    fs::write(&archive, b"tampered archive").unwrap();
    assert!(!verify_sha512_file(&archive, &integrity));
    assert!(decode_base64("not-a-sha512-digest").is_none());
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn prepared_cache_is_selected_only_when_it_covers_every_locked_archive() {
    let root = std::env::temp_dir().join(format!("koolade-npm-coverage-{}", uuid::Uuid::new_v4()));
    let cache = root.join("cache");
    fs::create_dir_all(&cache).unwrap();
    let package = b"locked package archive";
    let digest = Sha512::digest(package);
    let integrity = format!("sha512-{}", base64(&digest));
    fs::write(
        root.join("package-lock.json"),
        format!(
            r#"{{"packages":{{"node_modules/pkg":{{"resolved":"https://registry.npmjs.org/pkg/-/pkg-1.0.0.tgz","integrity":"{integrity}"}}}}}}"#
        ),
    )
    .unwrap();
    let content = npm_cache_digest_path(&cache, &integrity).unwrap();
    fs::create_dir_all(content.parent().unwrap()).unwrap();
    fs::write(&content, package).unwrap();
    assert!(cache_covers_lockfile(&root, &cache));
    assert!(super::verified_offline_cache(&root, &cache));
    fs::write(&content, b"tampered cache archive").unwrap();
    assert!(!super::verified_offline_cache(&root, &cache));
    fs::remove_file(content).unwrap();
    assert!(!super::verified_offline_cache(&root, &cache));
    assert!(!cache_covers_lockfile(&root, &cache));
    fs::remove_dir_all(root).unwrap();
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let a = chunk[0];
        let b = *chunk.get(1).unwrap_or(&0);
        let c = *chunk.get(2).unwrap_or(&0);
        out.push(TABLE[(a >> 2) as usize] as char);
        out.push(TABLE[(((a & 3) << 4) | (b >> 4)) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(((b & 15) << 2) | (c >> 6)) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(c & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
