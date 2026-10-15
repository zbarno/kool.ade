use flate2::{Compression, write::GzEncoder};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

const PACKAGE: &str = "koolade-cargo-fixture";
const VERSION: &str = "0.1.0";
const REGISTRY: &str = "index.crates.io-1949cf8c6b5b557f";

pub(super) struct CrateFixture {
    pub(super) archive: Vec<u8>,
    pub(super) checksum: String,
}

impl CrateFixture {
    pub(super) fn new() -> Self {
        let archive = {
            let encoder = GzEncoder::new(Vec::new(), Compression::default());
            let mut tar = tar::Builder::new(encoder);
            tar.mode(tar::HeaderMode::Deterministic);
            append_file(
                &mut tar,
                &format!("{PACKAGE}-{VERSION}/Cargo.toml"),
                "[package]\nname = \"koolade-cargo-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[lib]\npath = \"src/lib.rs\"\n",
            );
            append_file(
                &mut tar,
                &format!("{PACKAGE}-{VERSION}/src/lib.rs"),
                "pub fn answer() -> &'static str { \"restored\" }\n",
            );
            tar.finish().unwrap();
            tar.into_inner().unwrap().finish().unwrap()
        };
        let checksum = hex(&Sha256::digest(&archive));
        Self { archive, checksum }
    }

    pub(super) fn write_project(&self, root: &Path) {
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"cargo-restore-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nkoolade-cargo-fixture = \"=0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("Cargo.lock"),
            format!(
                "version = 4\n\n[[package]]\nname = \"cargo-restore-fixture\"\nversion = \"0.1.0\"\ndependencies = [\n \"koolade-cargo-fixture\",\n]\n\n[[package]]\nname = \"koolade-cargo-fixture\"\nversion = \"0.1.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"{}\"\n",
                self.checksum
            ),
        )
        .unwrap();
        fs::write(
            root.join("src/main.rs"),
            "fn main() { assert_eq!(koolade_cargo_fixture::answer(), \"restored\"); }\n",
        )
        .unwrap();
    }

    pub(super) fn write_registry_cache(&self, cargo_home: &Path) -> usize {
        let archive_path = cargo_home
            .join("registry/cache")
            .join(REGISTRY)
            .join(format!("{PACKAGE}-{VERSION}.crate"));
        let index_root = cargo_home
            .join("registry/index")
            .join(REGISTRY)
            .join(".cache/ko/ol");
        let index_paths = [
            index_root.join(PACKAGE),
            index_root.join(PACKAGE.replace('-', "_")),
        ];
        fs::create_dir_all(archive_path.parent().unwrap()).unwrap();
        fs::create_dir_all(index_root).unwrap();
        fs::write(&archive_path, &self.archive).unwrap();
        let index = self.sparse_index_entry();
        for index_path in index_paths {
            fs::write(index_path, &index).unwrap();
        }
        fs::write(
            cargo_home
                .join("registry/index")
                .join(REGISTRY)
                .join("config.json"),
            r#"{"dl":"https://static.crates.io/crates","api":"https://crates.io"}"#,
        )
        .unwrap();
        self.archive.len() + index.len()
    }

    fn sparse_index_entry(&self) -> Vec<u8> {
        let package = serde_json::json!({
            "name": PACKAGE,
            "vers": VERSION,
            "deps": [],
            "cksum": self.checksum,
            "features": {},
            "yanked": false,
        });
        let mut entry = b"\x03\x02\0\0\0etag: \"synthetic-cargo-index\"\0".to_vec();
        entry.extend_from_slice(VERSION.as_bytes());
        entry.push(0);
        entry.extend_from_slice(&serde_json::to_vec(&package).unwrap());
        entry.push(0);
        entry
    }
}

fn append_file<W: std::io::Write>(tar: &mut tar::Builder<W>, path: &str, contents: &str) {
    let mut header = tar::Header::new_gnu();
    header.set_size(contents.len() as u64);
    header.set_mode(0o644);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_cksum();
    tar.append_data(&mut header, path, contents.as_bytes())
        .unwrap();
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn synthetic_crates_io_fixture_builds_from_an_offline_cache() {
    let workspace = tempfile::tempdir().unwrap();
    let project = workspace.path().join("project");
    let cargo_home = workspace.path().join("cargo-home");
    let fixture = CrateFixture::new();
    fixture.write_project(&project);
    fixture.write_registry_cache(&cargo_home);
    let output = std::process::Command::new("cargo")
        .args(["+1.98.1", "build", "--locked", "--offline"])
        .current_dir(&project)
        .env("HOME", workspace.path())
        .env("CARGO_HOME", &cargo_home)
        .env("CARGO_REGISTRIES_CRATES_IO_PROTOCOL", "sparse")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "synthetic Cargo cache fixture did not resolve offline: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
