use std::io::Cursor;

use pgp::{composed::PublicOrSecret, types::KeyDetails};

use crate::app::bootstrap_remediation::trusted_assets::{
    DOCKER_DNF_KEY_FINGERPRINT, FEDORA_DNF_KEY_FINGERPRINT,
};

pub(super) fn verify_docker_signing_key(contents: &[u8]) -> anyhow::Result<()> {
    verify_pinned_signing_key(contents, DOCKER_DNF_KEY_FINGERPRINT, "Docker Fedora")
}

pub(super) fn verify_fedora_signing_key(contents: &[u8]) -> anyhow::Result<()> {
    verify_pinned_signing_key(contents, FEDORA_DNF_KEY_FINGERPRINT, "Fedora 45")
}

fn verify_pinned_signing_key(
    contents: &[u8],
    expected_fingerprint: &str,
    description: &str,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        !contents.is_empty() && contents.len() <= 1024 * 1024,
        "{description} signing key size is invalid"
    );
    let (keys, _) = PublicOrSecret::from_reader_many(Cursor::new(contents))
        .map_err(|error| anyhow::anyhow!("{description} signing key is malformed: {error}"))?;
    let mut fingerprints = Vec::new();
    for key in keys {
        let key = key.map_err(|error| {
            anyhow::anyhow!("{description} signing key contains a malformed key: {error}")
        })?;
        let PublicOrSecret::Public(key) = key else {
            anyhow::bail!("{description} signing key contains private key material")
        };
        fingerprints.push(format!("{:X}", key.fingerprint()));
    }
    anyhow::ensure!(
        fingerprints.len() == 1 && fingerprints[0] == expected_fingerprint,
        "{description} signing key does not contain only the pinned primary key"
    );
    Ok(())
}
