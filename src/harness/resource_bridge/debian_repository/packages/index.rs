use super::dependencies::{DependencyGroup, parse_dependencies};
use crate::execution::docker::image_extension::builder::{
    PackageArtifactIdentity, VerifiedPackageArtifact,
};
use crate::harness::resource_bridge::debian_repository::{SignedRelease, verify_index_bytes};
use flate2::read::MultiGzDecoder;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io::Read};

mod resolver;

const MAX_INDEX_BYTES: usize = 128 * 1024 * 1024;
const MAX_PACKAGE_STANZAS: usize = 250_000;

/// Records admitted only after the compressed index was checked against the
/// signed Release SHA256/size entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PackageRecord {
    pub name: String,
    pub version: String,
    pub architecture: String,
    pub multi_arch: Option<String>,
    pub filename: String,
    pub size: u64,
    pub sha256: String,
    pub(super) dependencies: Vec<DependencyGroup>,
    pub(super) provides: Vec<ProvidedPackage>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ProvidedPackage {
    pub name: String,
    pub version: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct PackageIndex {
    pub(super) records: BTreeMap<String, PackageRecord>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InstalledPackage {
    pub version: String,
    pub architecture: String,
    pub multi_arch: Option<String>,
}

impl PackageIndex {
    pub(crate) fn parse_gzip(
        release: &SignedRelease,
        path: &str,
        bytes: &[u8],
    ) -> anyhow::Result<Self> {
        verify_index_bytes(release, path, bytes)?;
        let decoder = MultiGzDecoder::new(bytes);
        let mut plain = Vec::new();
        decoder
            .take((MAX_INDEX_BYTES + 1) as u64)
            .read_to_end(&mut plain)?;
        anyhow::ensure!(
            plain.len() <= MAX_INDEX_BYTES,
            "Debian package index exceeds its size limit"
        );
        let text = std::str::from_utf8(&plain)?;
        let mut records = BTreeMap::new();
        let mut stanza = BTreeMap::<String, String>::new();
        let mut continuation: Option<String> = None;
        for line in text.lines().chain(std::iter::once("")) {
            if line.is_empty() {
                if !stanza.is_empty() {
                    let record = parse_record(&stanza)?;
                    anyhow::ensure!(
                        records.insert(record.name.clone(), record).is_none(),
                        "Debian index has duplicate package names"
                    );
                    anyhow::ensure!(
                        records.len() <= MAX_PACKAGE_STANZAS,
                        "Debian package index has too many records"
                    );
                    stanza.clear();
                }
                continuation = None;
                continue;
            }
            if line.starts_with(' ') || line.starts_with('\t') {
                let key = continuation.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Debian package index has orphan continuation")
                })?;
                stanza.get_mut(key).expect("field is present").push(' ');
                stanza
                    .get_mut(key)
                    .expect("field is present")
                    .push_str(line.trim());
                continue;
            }
            let (key, value) = line
                .split_once(':')
                .ok_or_else(|| anyhow::anyhow!("malformed Debian package field"))?;
            anyhow::ensure!(
                key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') && !key.is_empty(),
                "invalid Debian package field name"
            );
            anyhow::ensure!(
                stanza.insert(key.into(), value.trim().into()).is_none(),
                "duplicate Debian package field"
            );
            continuation = Some(key.into());
        }
        anyhow::ensure!(!records.is_empty(), "Debian package index is empty");
        Ok(Self { records })
    }
}

impl PackageRecord {
    pub(crate) fn source_url(&self) -> String {
        format!("https://deb.debian.org/debian/{}", self.filename)
    }

    pub(crate) fn verify_artifact(
        &self,
        bytes: Vec<u8>,
    ) -> anyhow::Result<VerifiedPackageArtifact> {
        use sha2::{Digest, Sha256};
        anyhow::ensure!(
            bytes.len() as u64 == self.size
                && format!("{:x}", Sha256::digest(&bytes)) == self.sha256,
            "Debian package artifact failed signed size or SHA256 check"
        );
        Ok(VerifiedPackageArtifact {
            identity: PackageArtifactIdentity {
                name: self.name.clone(),
                version: self.version.clone(),
                architecture: self.architecture.clone(),
                source: self.source_url(),
                sha256: self.sha256.clone(),
            },
            bytes,
        })
    }
}

pub(super) fn parse_record(fields: &BTreeMap<String, String>) -> anyhow::Result<PackageRecord> {
    let name = required(fields, "Package")?;
    let version = required(fields, "Version")?;
    let architecture = required(fields, "Architecture")?;
    let multi_arch = fields.get("Multi-Arch").map(String::as_str);
    let filename = required(fields, "Filename")?;
    let size = required(fields, "Size")?.parse::<u64>()?;
    let sha256 = required(fields, "SHA256")?.to_ascii_lowercase();
    anyhow::ensure!(
        valid_package_name(name) && !version.is_empty() && version.len() <= 128,
        "invalid Debian package identity"
    );
    anyhow::ensure!(
        architecture == "all" || architecture == "amd64" || architecture == "arm64",
        "unsupported Debian package architecture"
    );
    anyhow::ensure!(
        multi_arch.is_none_or(|value| matches!(value, "no" | "same" | "foreign" | "allowed")),
        "invalid Debian Multi-Arch field"
    );
    anyhow::ensure!(
        filename.starts_with("pool/main/")
            && filename
                .split('/')
                .all(|part| !part.is_empty() && part != "." && part != "..")
            && filename.ends_with(".deb"),
        "Debian package filename is outside approved pool/main"
    );
    anyhow::ensure!(
        size > 0
            && size <= 512 * 1024 * 1024
            && sha256.len() == 64
            && sha256.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid Debian package artifact integrity fields"
    );
    let mut dependencies = Vec::new();
    for value in [fields.get("Pre-Depends"), fields.get("Depends")]
        .into_iter()
        .flatten()
    {
        dependencies.extend(parse_dependencies(value)?);
    }
    let mut provides = Vec::new();
    if let Some(value) = fields.get("Provides") {
        for provided in value.split(',').map(str::trim) {
            let (name, version) = if let Some((name, version)) = provided.split_once('(') {
                let value = version
                    .strip_suffix(')')
                    .ok_or_else(|| anyhow::anyhow!("invalid Debian Provides version"))?
                    .trim();
                let (operator, version) = super::dependencies::parse_provided_version(value)?;
                anyhow::ensure!(
                    operator == "=",
                    "versioned Debian Provides must use equality"
                );
                (name.trim(), Some(version))
            } else {
                (provided, None)
            };
            anyhow::ensure!(
                valid_package_name(name),
                "invalid Debian Provides package name"
            );
            provides.push(ProvidedPackage {
                name: name.into(),
                version,
            });
        }
    }
    Ok(PackageRecord {
        name: name.into(),
        version: version.into(),
        architecture: architecture.into(),
        multi_arch: multi_arch.filter(|value| *value != "no").map(str::to_owned),
        filename: filename.into(),
        size,
        sha256,
        dependencies,
        provides,
    })
}

fn required<'a>(fields: &'a BTreeMap<String, String>, name: &str) -> anyhow::Result<&'a str> {
    fields
        .get(name)
        .map(String::as_str)
        .ok_or_else(|| anyhow::anyhow!("Debian package record is missing {name}"))
}

fn valid_package_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"+.-".contains(&b))
}
