//! Packet-owned execution metadata embedded as a small Markdown front matter.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const VERSION: u32 = 1;
const MAX_FRONTMATTER_BYTES: usize = 8192;
const MAX_DEPENDENCIES: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskMetadata {
    schema_version: u32,
    pub uid: String,
    pub batch_uid: String,
    pub repository_id: String,
    pub dependency_uids: Vec<String>,
}

impl TaskMetadata {
    pub fn new(
        identity: &crate::domain::ArtifactIdentity,
        repository_id: &str,
        dependency_uids: Vec<String>,
    ) -> anyhow::Result<Self> {
        let metadata = Self {
            schema_version: VERSION,
            uid: identity.uid.clone(),
            batch_uid: identity
                .parent_uid
                .clone()
                .ok_or_else(|| anyhow::anyhow!("Generated task identity has no batch identity"))?,
            repository_id: repository_id.into(),
            dependency_uids,
        };
        metadata.validate(Some(identity))?;
        Ok(metadata)
    }

    pub fn validate(
        &self,
        identity: Option<&crate::domain::ArtifactIdentity>,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.schema_version == VERSION,
            "Unsupported task metadata version {}",
            self.schema_version
        );
        anyhow::ensure!(
            uuid::Uuid::parse_str(&self.uid).is_ok(),
            "Task metadata has an invalid UID"
        );
        anyhow::ensure!(
            uuid::Uuid::parse_str(&self.batch_uid).is_ok(),
            "Task metadata has an invalid batch UID"
        );
        anyhow::ensure!(
            valid_repository_id(&self.repository_id),
            "Task metadata has an invalid repository ID"
        );
        anyhow::ensure!(
            self.dependency_uids.len() <= MAX_DEPENDENCIES,
            "Task metadata has too many dependencies"
        );
        let mut dependencies = BTreeSet::new();
        for uid in &self.dependency_uids {
            anyhow::ensure!(
                uuid::Uuid::parse_str(uid).is_ok(),
                "Task metadata has an invalid dependency UID"
            );
            anyhow::ensure!(uid != &self.uid, "A task cannot depend on itself");
            anyhow::ensure!(
                dependencies.insert(uid),
                "Task metadata repeats a dependency UID"
            );
        }
        if let Some(identity) = identity {
            anyhow::ensure!(
                self.uid == identity.uid,
                "Task metadata UID does not match its artifact identity"
            );
            anyhow::ensure!(
                identity.parent_uid.as_deref() == Some(self.batch_uid.as_str()),
                "Task metadata batch does not match its artifact identity"
            );
        }
        Ok(())
    }
}

pub fn parse(markdown: &str) -> anyhow::Result<Option<TaskMetadata>> {
    let Some((frontmatter, _)) = frontmatter(markdown)? else {
        return Ok(None);
    };
    anyhow::ensure!(
        frontmatter.len() <= MAX_FRONTMATTER_BYTES,
        "Task front matter is too large"
    );
    let mut lines = frontmatter.lines();
    let field = lines.next().unwrap_or_default();
    let Some(json) = field.strip_prefix("packet-task: ") else {
        return Ok(None);
    };
    anyhow::ensure!(
        lines.all(|line| line.trim().is_empty()),
        "Unexpected field in Packet task front matter"
    );
    let metadata: TaskMetadata = serde_json::from_str(json)?;
    metadata.validate(None)?;
    Ok(Some(metadata))
}

pub fn embed(markdown: &str, metadata: &TaskMetadata) -> anyhow::Result<String> {
    metadata.validate(None)?;
    let body = frontmatter(markdown)?.map_or(markdown, |(_, body)| body);
    let value = serde_json::to_string(metadata)?;
    Ok(format!("---\npacket-task: {value}\n---\n\n{body}"))
}

pub fn visible_content(markdown: &str) -> String {
    let body = frontmatter(markdown)
        .ok()
        .flatten()
        .map_or(markdown, |(_, body)| body);
    crate::domain::ArtifactIdentity::visible_markdown(body)
}

/// Read legacy relative dependency links only from their designated section.
/// The caller additionally checks that each target is a same-batch story.
pub fn legacy_dependencies(markdown: &str) -> anyhow::Result<Vec<String>> {
    use pulldown_cmark::{Event, Parser, Tag};
    let mut active = false;
    let mut section = String::new();
    for line in markdown.lines() {
        if line.starts_with("## ") {
            active = line.trim().eq_ignore_ascii_case("## Dependencies");
            continue;
        }
        if active {
            section.push_str(line);
            section.push('\n');
        }
    }
    let mut links = BTreeSet::new();
    for event in Parser::new(&section) {
        if let Event::Start(Tag::Link { dest_url, .. }) = event {
            let target = dest_url.to_string();
            let path = std::path::Path::new(&target);
            anyhow::ensure!(
                path.components().count() == 1 && target.ends_with(".md"),
                "unsupported legacy dependency link {target}"
            );
            links.insert(target);
        }
    }
    anyhow::ensure!(
        links.len() <= MAX_DEPENDENCIES,
        "Legacy task has too many dependency links"
    );
    Ok(links.into_iter().collect())
}

fn valid_repository_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 40
        && id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn frontmatter(markdown: &str) -> anyhow::Result<Option<(&str, &str)>> {
    let mut lines = markdown.split_inclusive('\n');
    let Some(first) = lines.next() else {
        return Ok(None);
    };
    if first.trim_end_matches(['\r', '\n']) != "---" {
        return Ok(None);
    }
    let mut offset = first.len();
    for line in lines {
        if line.trim_end_matches(['\r', '\n']) == "---" {
            let start = first.len();
            let end = offset;
            let mut body_start = offset + line.len();
            let body_bytes = markdown.as_bytes();
            if body_bytes.get(body_start) == Some(&b'\r') {
                body_start += 1;
            }
            if body_bytes.get(body_start) == Some(&b'\n') {
                body_start += 1;
            }
            return Ok(Some((&markdown[start..end], &markdown[body_start..])));
        }
        offset += line.len();
        anyhow::ensure!(
            offset <= MAX_FRONTMATTER_BYTES,
            "Task front matter is too large"
        );
    }
    anyhow::bail!("Task front matter is missing its closing delimiter")
}

#[cfg(test)]
#[path = "metadata_tests.rs"]
mod tests;
