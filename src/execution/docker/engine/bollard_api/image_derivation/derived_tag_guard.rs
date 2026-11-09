//! Fail-safe disposition of app-owned derived image tags whose identity
//! verification fails.
//!
//! Tags shaped `koolade-derived:<recipe digest>` are reserved by this
//! application: an existing tag whose image lacks the builder identity
//! labels is a cache collision or deliberate poisoning attempt. The tag is
//! content-addressed, so trusting it without re-derivation would amount to
//! executing attacker-chosen image bytes under a verified name.

/// Decision for a derived image tag that failed identity verification.
pub(crate) enum DerivedTagDisposition {
    /// Another tag retains the image, so releasing the application-owned
    /// tag (non-forced) discards only that name and the build can recreate
    /// it. Repo digest references never qualify: the engine deletes the
    /// image when its last tag is removed, expiring those references too.
    UntagAndRebuild,
    /// The application-owned tag is the only reference to an unverified
    /// image (or the identifier is malformed). Refuse destructive removal
    /// and fail closed with an actionable error instead.
    RefuseRemoval,
}

pub(crate) fn classify_foreign_tag(
    image_id: &str,
    other_tag_reference_exists: bool,
) -> DerivedTagDisposition {
    let well_formed = image_id.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    });
    if well_formed && other_tag_reference_exists {
        DerivedTagDisposition::UntagAndRebuild
    } else {
        DerivedTagDisposition::RefuseRemoval
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(digit: char) -> String {
        format!("sha256:{}", digit.to_string().repeat(64))
    }

    #[test]
    fn foreign_image_retained_by_other_reference_can_be_untagged() {
        assert!(matches!(
            classify_foreign_tag(&digest('1'), true),
            DerivedTagDisposition::UntagAndRebuild
        ));
    }

    #[test]
    fn sole_reference_to_foreign_image_is_refused() {
        assert!(matches!(
            classify_foreign_tag(&digest('2'), false),
            DerivedTagDisposition::RefuseRemoval
        ));
    }

    #[test]
    fn malformed_identifier_is_never_trusted() {
        let mut suspects: Vec<String> =
            vec![String::new(), "sha256:".to_owned(), "sha256:abc".to_owned()];
        suspects.push(format!("sha256:{}zz", "0".repeat(64)));
        for suspect in suspects {
            assert!(
                matches!(
                    classify_foreign_tag(&suspect, true),
                    DerivedTagDisposition::RefuseRemoval
                ),
                "{suspect}"
            );
        }
    }
}
