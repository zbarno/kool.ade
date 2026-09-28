use super::super::ChangeStatus;
use super::{ChangeMetadata, replace_metadata};
use crate::domain::ArtifactIdentity;

impl ChangeMetadata {
    pub fn rewrite_markdown(
        markdown: &str,
        identity: &ArtifactIdentity,
        status: ChangeStatus,
        previous: Option<&Self>,
    ) -> anyhow::Result<String> {
        let written = Self::write_markdown(markdown, identity, status)?;
        let Some(previous) = previous else {
            return Ok(written);
        };
        anyhow::ensure!(
            previous.uid == identity.uid && previous.display_id == identity.display_id,
            "Existing change metadata identity does not match the rewritten artifact"
        );
        let mut metadata = previous.clone();
        metadata.status = status;
        replace_metadata(&written, &metadata)
    }
}
