//! Bounded reader for Cargo lockfiles limited to checksum-backed crates.io entries.
use std::{collections::BTreeMap, fs, path::Path};

const MAX_LOCKFILE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_LOCKED_PACKAGES: usize = 2_000;
const CRATES_IO_SOURCE: &str = "registry+https://github.com/rust-lang/crates.io-index";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LockedPackage {
    pub(super) name: String,
    pub(super) version: String,
    pub(super) checksum: String,
}

pub(super) fn collect(root: &Path) -> anyhow::Result<Vec<LockedPackage>> {
    let path = root.join("Cargo.lock");
    let metadata = fs::symlink_metadata(&path)?;
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "Cargo.lock must be a regular file"
    );
    anyhow::ensure!(
        metadata.len() <= MAX_LOCKFILE_BYTES,
        "Cargo.lock exceeds the 32 MiB scan limit"
    );
    let contents = fs::read_to_string(path)?;
    collect_contents(&contents)
}

pub(super) fn package_identities_from_contents(
    contents: &str,
) -> anyhow::Result<Vec<crate::harness::DependencyPackageIdentity>> {
    Ok(collect_contents(contents)?
        .into_iter()
        .map(|package| crate::harness::DependencyPackageIdentity {
            package: package.name,
            version: package.version,
            source: "https://index.crates.io".into(),
            integrity: format!("sha256:{}", package.checksum),
        })
        .collect())
}

fn collect_contents(contents: &str) -> anyhow::Result<Vec<LockedPackage>> {
    anyhow::ensure!(
        contents.len() as u64 <= MAX_LOCKFILE_BYTES,
        "Cargo.lock exceeds the 32 MiB scan limit"
    );
    let mut packages = BTreeMap::new();
    for block in contents.split("[[package]]").skip(1) {
        let mut name = None;
        let mut version = None;
        let mut source = None;
        let mut checksum = None;
        for line in block.lines().map(str::trim) {
            if line.starts_with("name = ") {
                name = Some(parse_quoted_value(line, "name")?);
            } else if line.starts_with("version = ") {
                version = Some(parse_quoted_value(line, "version")?);
            } else if line.starts_with("source = ") {
                source = Some(parse_quoted_value(line, "source")?);
            } else if line.starts_with("checksum = ") {
                checksum = Some(parse_quoted_value(line, "checksum")?);
            }
        }
        let Some(source) = source else {
            continue;
        };
        anyhow::ensure!(
            source == CRATES_IO_SOURCE,
            "Cargo.lock includes a non-crates.io package source"
        );
        let name = name.ok_or_else(|| anyhow::anyhow!("Cargo.lock package name is missing"))?;
        let version =
            version.ok_or_else(|| anyhow::anyhow!("Cargo.lock package version is missing"))?;
        let checksum =
            checksum.ok_or_else(|| anyhow::anyhow!("Cargo.lock package checksum is missing"))?;
        anyhow::ensure!(
            valid_name(&name) && valid_version(&version) && valid_checksum(&checksum),
            "Cargo.lock contains an invalid package identity or checksum"
        );
        packages.insert(
            (name.clone(), version.clone()),
            LockedPackage {
                name,
                version,
                checksum,
            },
        );
        anyhow::ensure!(
            packages.len() <= MAX_LOCKED_PACKAGES,
            "Cargo.lock contains more than {MAX_LOCKED_PACKAGES} registry packages"
        );
    }
    Ok(packages.into_values().collect())
}

fn parse_quoted_value(line: &str, key: &str) -> anyhow::Result<String> {
    let value = line
        .strip_prefix(key)
        .and_then(|line| line.strip_prefix(" = \""))
        .and_then(|line| line.strip_suffix('"'))
        .ok_or_else(|| anyhow::anyhow!("Cargo.lock contains a malformed {key} field"))?;
    anyhow::ensure!(
        !value.contains('\\') && !value.chars().any(char::is_control),
        "Cargo.lock contains an escaped or invalid {key} field"
    );
    Ok(value.to_owned())
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
}

fn valid_version(version: &str) -> bool {
    !version.is_empty()
        && version.len() <= 128
        && version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b".+-".contains(&byte))
}

fn valid_checksum(checksum: &str) -> bool {
    checksum.len() == 64 && checksum.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_only_checksum_backed_crates_io_packages() {
        let root =
            std::env::temp_dir().join(format!("koolade-cargo-lock-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("Cargo.lock"),
            format!(
                "[[package]]\nname = \"serde\"\nversion = \"1.0.0\"\nsource = \"{CRATES_IO_SOURCE}\"\nchecksum = \"{}\"\n\n[[package]]\nname = \"local\"\nversion = \"0.1.0\"\n",
                "a".repeat(64)
            ),
        )
        .unwrap();
        assert_eq!(
            collect(&root).unwrap(),
            vec![LockedPackage {
                name: "serde".into(),
                version: "1.0.0".into(),
                checksum: "a".repeat(64),
            }]
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_git_private_and_checksumless_registry_packages() {
        let root =
            std::env::temp_dir().join(format!("koolade-cargo-lock-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        for source in [
            "git+https://github.com/example/package#abc",
            "registry+https://packages.example.com/index",
        ] {
            fs::write(
                root.join("Cargo.lock"),
                format!("[[package]]\nname = \"pkg\"\nversion = \"1.0.0\"\nsource = \"{source}\"\nchecksum = \"{}\"\n", "a".repeat(64)),
            )
            .unwrap();
            assert!(collect(&root).is_err());
        }
        fs::write(
            root.join("Cargo.lock"),
            format!(
                "[[package]]\nname = \"pkg\"\nversion = \"1.0.0\"\nsource = \"{CRATES_IO_SOURCE}\"\n"
            ),
        )
        .unwrap();
        assert!(collect(&root).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
