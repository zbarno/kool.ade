//! Stable identity embedded in durable Markdown artifacts.
use serde::{Deserialize, Serialize};

const MARKER: &str = "<!-- koolade-artifact-id:v1 ";

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactIdentity {
    pub uid: String,
    pub display_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_uid: Option<String>,
}

impl ArtifactIdentity {
    pub fn new(display_id: &str, title: &str) -> Self {
        Self {
            uid: uuid::Uuid::new_v4().hyphenated().to_string(),
            display_id: display_id.to_owned(),
            title: title.to_owned(),
            parent_uid: None,
        }
    }

    pub fn from_markdown(markdown: &str) -> anyhow::Result<Option<Self>> {
        let mut found = None;
        for line in markdown.lines().filter(|line| line.starts_with(MARKER)) {
            anyhow::ensure!(
                found.is_none(),
                "Markdown contains duplicate Koolade identities"
            );
            let json = line
                .strip_prefix(MARKER)
                .and_then(|line| line.strip_suffix(" -->"))
                .ok_or_else(|| anyhow::anyhow!("Malformed Koolade identity marker"))?;
            let identity: Self = serde_json::from_str(json)?;
            anyhow::ensure!(
                uuid::Uuid::parse_str(&identity.uid).is_ok()
                    && identity
                        .parent_uid
                        .as_deref()
                        .is_none_or(|uid| uuid::Uuid::parse_str(uid).is_ok())
                    && !identity.display_id.trim().is_empty()
                    && !identity.title.trim().is_empty(),
                "Malformed Koolade identity fields"
            );
            found = Some(identity);
        }
        Ok(found)
    }

    pub fn visible_markdown(markdown: &str) -> String {
        let mut content = markdown
            .lines()
            .filter(|line| !line.starts_with(MARKER))
            .collect::<Vec<_>>()
            .join("\n");
        if markdown.ends_with('\n') {
            content.push('\n');
        }
        content
    }

    /// Embed a new identity or preserve the existing one while updating its title.
    pub fn preserve_markdown(
        markdown: &str,
        previous: Option<&str>,
        display_id: &str,
        title: &str,
    ) -> anyhow::Result<String> {
        Self::preserve_markdown_with_parent(markdown, previous, display_id, title, None)
    }

    pub fn preserve_markdown_with_parent(
        markdown: &str,
        previous: Option<&str>,
        display_id: &str,
        title: &str,
        parent_uid: Option<&str>,
    ) -> anyhow::Result<String> {
        let current = Self::from_markdown(markdown)?;
        let old = previous.map(Self::from_markdown).transpose()?.flatten();
        anyhow::ensure!(
            old.is_some() || current.is_none(),
            "Only Koolade can assign a new artifact identity"
        );
        if let (Some(current), Some(old)) = (&current, &old) {
            anyhow::ensure!(
                current.uid == old.uid,
                "Feature identity changed during update"
            );
        }
        let mut identity = current
            .or(old)
            .unwrap_or_else(|| Self::new(display_id, title));
        anyhow::ensure!(
            identity.display_id == display_id,
            "Artifact display ID cannot change during update"
        );
        if let Some(parent_uid) = parent_uid {
            anyhow::ensure!(
                uuid::Uuid::parse_str(parent_uid).is_ok()
                    && identity
                        .parent_uid
                        .as_deref()
                        .is_none_or(|old| old == parent_uid),
                "Artifact parent identity cannot change during update"
            );
            identity.parent_uid = Some(parent_uid.to_owned());
        }
        identity.title = title.to_owned();
        let marker = format!("{MARKER}{} -->", serde_json::to_string(&identity)?);
        let mut lines = markdown
            .lines()
            .filter(|line| !line.starts_with(MARKER))
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let index = lines
            .iter()
            .position(|line| !line.trim().is_empty())
            .map_or(0, |index| index + 1);
        lines.insert(index, marker);
        if lines
            .get(index + 1)
            .is_some_and(|line| !line.trim().is_empty())
        {
            lines.insert(index + 1, String::new());
        }
        let mut result = lines.join("\n");
        if markdown.ends_with('\n') {
            result.push('\n');
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feature_uid_survives_title_edit_and_content_move() {
        let original = "# F10: Cache responses\n\n## Intent\n\nAvoid duplicate work.\n";
        let created =
            ArtifactIdentity::preserve_markdown(original, None, "F10", "Cache responses").unwrap();
        let before = ArtifactIdentity::from_markdown(&created).unwrap().unwrap();
        let renamed = created.replace("F10: Cache responses", "F10: Reuse cached responses");
        let moved = ArtifactIdentity::preserve_markdown(
            &renamed,
            Some(&created),
            "F10",
            "Reuse cached responses",
        )
        .unwrap();
        let after = ArtifactIdentity::from_markdown(&moved).unwrap().unwrap();
        assert_eq!(after.uid, before.uid);
        assert_eq!(after.display_id, "F10");
        assert_eq!(after.title, "Reuse cached responses");
        assert!(moved.contains("Avoid duplicate work."));
    }

    #[test]
    fn conflicting_identity_is_rejected_and_new_artifacts_are_distinct() {
        let first = ArtifactIdentity::preserve_markdown("# F1: One\n", None, "F1", "One").unwrap();
        let second = ArtifactIdentity::preserve_markdown("# F2: Two\n", None, "F2", "Two").unwrap();
        assert_ne!(
            ArtifactIdentity::from_markdown(&first)
                .unwrap()
                .unwrap()
                .uid,
            ArtifactIdentity::from_markdown(&second)
                .unwrap()
                .unwrap()
                .uid
        );
        let conflict = ArtifactIdentity::preserve_markdown(&second, Some(&first), "F2", "Two");
        assert!(conflict.is_err());
        let invented = ArtifactIdentity::new("F3", "Three");
        let untrusted = format!(
            "# F3: Three\n<!-- koolade-artifact-id:v1 {} -->\n",
            serde_json::to_string(&invented).unwrap()
        );
        assert!(ArtifactIdentity::preserve_markdown(&untrusted, None, "F3", "Three").is_err());
    }
}
