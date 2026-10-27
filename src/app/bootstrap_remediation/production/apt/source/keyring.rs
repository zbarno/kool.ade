use std::io::Cursor;

use pgp::{composed::PublicOrSecret, types::KeyDetails};

const DOCKER_PRIMARY_FINGERPRINT: &str = "9DC858229FC7DD38854AE2D88D81803C0EBFCD88";

pub(super) fn verify_docker_signing_key(contents: &[u8]) -> anyhow::Result<()> {
    verify_keyring_fingerprints(contents, &[DOCKER_PRIMARY_FINGERPRINT])
}

fn verify_keyring_fingerprints(contents: &[u8], expected: &[&str]) -> anyhow::Result<()> {
    anyhow::ensure!(
        !contents.is_empty() && contents.len() <= 1024 * 1024,
        "Docker APT signing keyring size is invalid"
    );
    let (keys, _) = PublicOrSecret::from_reader_many(Cursor::new(contents))
        .map_err(|error| anyhow::anyhow!("Docker APT signing keyring is malformed: {error}"))?;
    let mut fingerprints = Vec::new();
    for key in keys {
        let key = key.map_err(|error| {
            anyhow::anyhow!("Docker APT signing keyring contains a malformed key: {error}")
        })?;
        let PublicOrSecret::Public(key) = key else {
            anyhow::bail!("Docker APT signing keyring contains private key material")
        };
        fingerprints.push(format!("{:X}", key.fingerprint()));
    }
    let mut actual = fingerprints;
    let mut expected = expected
        .iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>();
    actual.sort();
    expected.sort();
    anyhow::ensure!(
        !actual.is_empty() && actual == expected,
        "Docker APT signing keyring does not contain only approved Docker signing keys"
    );
    Ok(())
}

#[cfg(test)]
#[path = "keyring_tests.rs"]
mod tests;
