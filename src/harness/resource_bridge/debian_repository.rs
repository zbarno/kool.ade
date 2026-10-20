//! Trust verification for the first supported system package repository.
//!
//! The boundary here is the broker: it accepts only a Debian Bookworm
//! `InRelease` signed by the bundled Debian archive keys. Tests use a synthetic
//! signed release and test key. This unit proof does not replace the future
//! Docker boundary test for offline installation (Docker is an external setup
//! cost and is required for that test).

use chrono::{DateTime, Utc};
use pgp::{
    composed::{CleartextSignedMessage, Deserializable, SignedPublicKey},
    types::KeyDetails,
};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Cursor};

mod key_bindings;

const MAX_INRELEASE_BYTES: usize = 16 * 1024 * 1024;
const BOOKWORM_KEYRING: &[u8] =
    include_bytes!("../../../containers/keys/debian-bookworm-archive-keyring.gpg");
const BOOKWORM_FINGERPRINTS: &[&str] = &[
    "B8B80B5B623EAB6AD8775C45B7C5D7D6350947F8",
    "04B54C3CDCA79751B16BC6B5225629DF75B188BD",
    "4D64FEC119C2029067D6E791F8D2585B8783D481",
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SignedRelease {
    pub origin: String,
    pub label: String,
    pub suite: String,
    pub codename: String,
    pub valid_until_unix: i64,
    pub architectures: Vec<String>,
    pub components: Vec<String>,
    pub sha256: BTreeMap<String, (String, u64)>,
}

pub(crate) fn verify_bookworm_inrelease(bytes: &[u8]) -> anyhow::Result<SignedRelease> {
    verify_inrelease_with_keyring_at(bytes, BOOKWORM_KEYRING, BOOKWORM_FINGERPRINTS, Utc::now())
}

fn verify_inrelease_with_keyring_at(
    bytes: &[u8],
    keyring: &[u8],
    expected_fingerprints: &[&str],
    now: DateTime<Utc>,
) -> anyhow::Result<SignedRelease> {
    anyhow::ensure!(
        !bytes.is_empty() && bytes.len() <= MAX_INRELEASE_BYTES,
        "Debian InRelease size is invalid"
    );
    anyhow::ensure!(!keyring.is_empty(), "Debian archive keyring is unavailable");
    let inrelease = std::str::from_utf8(bytes)?;
    let (message, _) = CleartextSignedMessage::from_string(inrelease).map_err(|error| {
        anyhow::anyhow!("invalid Debian InRelease signature framing: {error:#}")
    })?;
    let keys = SignedPublicKey::from_bytes_many(Cursor::new(keyring))
        .map_err(|error| anyhow::anyhow!("invalid Debian archive keyring: {error:#}"))?;
    let mut verified = None;
    for key in keys {
        let key = key.map_err(|error| anyhow::anyhow!("invalid Debian archive key: {error:#}"))?;
        let fingerprint = format!("{:X}", key.fingerprint());
        if !expected_fingerprints.contains(&fingerprint.as_str()) {
            continue;
        }
        key_bindings::verify_self_bindings(&key)
            .map_err(|error| anyhow::anyhow!("invalid Debian archive key bindings: {error}"))?;
        if message.verify(&key).is_ok() {
            anyhow::ensure!(
                verified.is_none(),
                "Debian InRelease has ambiguous approved signers"
            );
            verified = Some(message.signed_text());
        }
    }
    let body = verified.ok_or_else(|| {
        anyhow::anyhow!("Debian InRelease signature did not verify with an approved archive key")
    })?;
    parse_release_body_at(&body, now)
}

fn parse_release_body_at(body: &str, now: DateTime<Utc>) -> anyhow::Result<SignedRelease> {
    let mut fields = BTreeMap::<String, String>::new();
    let mut continuation: Option<String> = None;
    for line in body.lines() {
        let line = line.strip_prefix("- ").unwrap_or(line);
        if line.as_bytes().first().is_some_and(u8::is_ascii_whitespace) {
            let key = continuation
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Debian Release has an orphan continuation"))?;
            fields
                .get_mut(key)
                .expect("continuation key was inserted")
                .push('\n');
            fields
                .get_mut(key)
                .expect("continuation key was inserted")
                .push_str(line.trim());
            continue;
        }
        let (key, value) = line
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("Debian Release contains a malformed field"))?;
        anyhow::ensure!(
            !key.is_empty()
                && key
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                && fields
                    .insert(key.to_owned(), value.trim().to_owned())
                    .is_none(),
            "Debian Release contains a duplicate or invalid field"
        );
        continuation = Some(key.to_owned());
    }
    let release = SignedRelease {
        origin: field(&fields, "Origin")?,
        label: field(&fields, "Label")?,
        suite: field(&fields, "Suite")?,
        codename: field(&fields, "Codename")?,
        valid_until_unix: validate_release_window(
            field(&fields, "Date")?.as_str(),
            fields.get("Valid-Until").map(String::as_str),
            now,
        )?,
        architectures: field(&fields, "Architectures")?
            .split_ascii_whitespace()
            .map(str::to_owned)
            .collect(),
        components: field(&fields, "Components")?
            .split_ascii_whitespace()
            .map(str::to_owned)
            .collect(),
        sha256: parse_sha256(fields.get("SHA256").map(String::as_str).unwrap_or_default())?,
    };
    anyhow::ensure!(
        release.origin == "Debian"
            && release.label == "Debian"
            // The codename is the immutable release identity. Debian updates
            // Suite to oldstable/oldoldstable as releases age, including on
            // the versioned dists/bookworm endpoint.
            && matches!(release.suite.as_str(), "bookworm" | "oldstable" | "oldoldstable")
            && release.codename == "bookworm"
            && release
                .components
                .iter()
                .any(|component| component == "main")
            && release.architectures.iter().any(|arch| arch == "amd64")
            && release.architectures.iter().any(|arch| arch == "arm64"),
        "Debian Release does not match the approved Bookworm repository"
    );
    Ok(release)
}

fn validate_release_window(
    date: &str,
    valid_until: Option<&str>,
    now: DateTime<Utc>,
) -> anyhow::Result<i64> {
    let date = parse_debian_date(date)?.with_timezone(&Utc);
    // Debian stable Release files can omit Valid-Until and may be published
    // infrequently. Accept at most 180 days from the signed Date in that case;
    // the finite cap accommodates Bookworm's observed ~97-day metadata age.
    // When the repository supplies Valid-Until, enforce that signed value and
    // retain the tighter 10-day maximum span.
    let supplied_expiry = valid_until.is_some();
    let valid_until = valid_until
        .map(parse_debian_date)
        .transpose()?
        .map(|date| date.with_timezone(&Utc))
        .unwrap_or_else(|| date + chrono::Duration::days(180));
    let max_span = if supplied_expiry { 10 } else { 180 };
    anyhow::ensure!(
        date <= now + chrono::Duration::minutes(5)
            && valid_until > now
            && valid_until > date
            && valid_until - date <= chrono::Duration::days(max_span),
        "Debian Release metadata is expired or outside the accepted freshness window"
    );
    Ok(valid_until.timestamp())
}

fn parse_debian_date(value: &str) -> anyhow::Result<DateTime<chrono::FixedOffset>> {
    let normalized;
    let value = if let Some(date) = value.strip_suffix(" UTC") {
        normalized = format!("{date} +0000");
        normalized.as_str()
    } else {
        value
    };
    Ok(DateTime::parse_from_rfc2822(value)?)
}

pub(crate) fn verify_index_bytes(
    release: &SignedRelease,
    path: &str,
    bytes: &[u8],
) -> anyhow::Result<()> {
    let (digest, size) = release.sha256.get(path).ok_or_else(|| {
        anyhow::anyhow!("Debian package index is not covered by the signed Release")
    })?;
    anyhow::ensure!(
        *size == bytes.len() as u64 && format!("{:x}", Sha256::digest(bytes)) == *digest,
        "Debian package index failed its signed size or SHA256 check"
    );
    Ok(())
}

fn parse_sha256(value: &str) -> anyhow::Result<BTreeMap<String, (String, u64)>> {
    let mut entries = BTreeMap::new();
    for line in value.lines().filter(|line| !line.trim().is_empty()) {
        let mut fields = line.split_ascii_whitespace();
        let digest = fields.next().unwrap_or_default();
        let size = fields.next().unwrap_or_default();
        let path = fields.next().unwrap_or_default();
        anyhow::ensure!(
            digest.len() == 64
                && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
                && !path.is_empty()
                && !path.starts_with('/')
                && !path.split('/').any(|part| part == "..")
                && fields.next().is_none(),
            "Debian Release has an invalid SHA256 entry (digest length {}, size {:?}, path {:?})",
            digest.len(),
            size,
            path
        );
        let size = size.parse::<u64>()?;
        anyhow::ensure!(
            entries
                .insert(path.to_owned(), (digest.to_ascii_lowercase(), size))
                .is_none(),
            "Debian Release has a duplicate package index"
        );
    }
    anyhow::ensure!(!entries.is_empty(), "Debian Release has no SHA256 entries");
    Ok(entries)
}

fn field(fields: &BTreeMap<String, String>, name: &str) -> anyhow::Result<String> {
    fields
        .get(name)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Debian Release is missing {name}"))
}

#[cfg(test)]
#[path = "debian_repository/tests.rs"]
mod tests;

mod packages;
pub(crate) use packages::{InstalledPackage, RepositoryPackagePlan};
