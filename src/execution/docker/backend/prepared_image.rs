use crate::execution::docker::images::SelectedImage;

/// Proof that the backend registry accepted this exact derived image and the
/// Docker daemon observed its pinned identity before container creation.
pub(in crate::execution::docker) struct PreparedDerivedImage {
    pub(in crate::execution::docker) image: SelectedImage,
    pub(in crate::execution::docker) content_identity: String,
    pub(in crate::execution::docker) base_digest: String,
}

impl PreparedDerivedImage {
    pub(in crate::execution::docker) fn matches(&self, image: &SelectedImage) -> bool {
        self.image == *image
            && valid_sha256(&self.content_identity)
            && valid_sha256(&self.base_digest)
    }

    #[cfg(test)]
    pub(in crate::execution::docker) fn test_approval(
        image: SelectedImage,
        base_digest: String,
    ) -> Self {
        Self {
            image,
            content_identity: format!("sha256:{}", "a".repeat(64)),
            base_digest,
        }
    }
}

fn valid_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}
