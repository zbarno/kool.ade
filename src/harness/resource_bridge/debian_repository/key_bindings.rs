use pgp::{composed::SignedPublicKey, packet::Signature, types::KeyDetails};

/// Verify self-certifications and subkey bindings on a pinned certificate.
///
/// Debian archive certificates can include third-party certifications whose
/// issuer is not part of the archive keyring. Those certifications do not
/// establish the pinned key's identity and must not prevent checking its own
/// bindings. Signatures without issuer metadata are retained and must verify
/// cryptographically as self-signatures.
pub(super) fn verify_self_bindings(key: &SignedPublicKey) -> pgp::errors::Result<()> {
    let own_key_id = key.legacy_key_id();
    let own_fingerprint = key.fingerprint();
    let mut self_checked = key.clone();

    for user in &mut self_checked.details.users {
        user.signatures
            .retain(|signature| is_self_signature(signature, &own_key_id, &own_fingerprint));
    }
    for attribute in &mut self_checked.details.user_attributes {
        attribute
            .signatures
            .retain(|signature| is_self_signature(signature, &own_key_id, &own_fingerprint));
    }

    self_checked.verify_bindings()
}

fn is_self_signature(
    signature: &Signature,
    own_key_id: &pgp::types::KeyId,
    own_fingerprint: &pgp::types::Fingerprint,
) -> bool {
    let issuer_ids = signature.issuer_key_id();
    let issuer_fingerprints = signature.issuer_fingerprint();
    (issuer_ids.is_empty() && issuer_fingerprints.is_empty())
        || issuer_ids.contains(&own_key_id)
        || issuer_fingerprints.contains(&own_fingerprint)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pgp::composed::{Deserializable, SignedPublicKey};
    use std::io::Cursor;

    #[test]
    fn approved_debian_keys_have_valid_self_bindings() {
        let keys =
            SignedPublicKey::from_bytes_many(Cursor::new(super::super::BOOKWORM_KEYRING)).unwrap();
        let mut approved = 0;
        for key in keys {
            let key = key.unwrap();
            let fingerprint = format!("{:X}", key.fingerprint());
            if super::super::BOOKWORM_FINGERPRINTS.contains(&fingerprint.as_str()) {
                verify_self_bindings(&key).unwrap();
                approved += 1;
            }
        }
        assert_eq!(approved, super::super::BOOKWORM_FINGERPRINTS.len());
    }

    #[test]
    fn missing_pinned_self_binding_is_rejected() {
        let mut keys =
            SignedPublicKey::from_bytes_many(Cursor::new(super::super::BOOKWORM_KEYRING)).unwrap();
        let mut key = keys.next().unwrap().unwrap();
        key.details.users[0].signatures.clear();
        assert!(verify_self_bindings(&key).is_err());
    }
}
