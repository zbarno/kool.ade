//! Koolade-owned execution metadata embedded as a small Markdown front matter.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

mod legacy;
mod routing;

pub use legacy::legacy_dependencies;

const VERSION: u32 = 5;
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
    /// Standalone task UID that owns this internal subtask, when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_task_uid: Option<String>,
    #[serde(
        default,
        alias = "source_branch",
        skip_serializing_if = "Option::is_none"
    )]
    pub source_branch: Option<String>,
    #[serde(
        default,
        alias = "destination_branch",
        skip_serializing_if = "Option::is_none"
    )]
    pub destination_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parallelizable: Option<bool>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub routing_overrides:
        std::collections::BTreeMap<String, crate::persistence::harness_settings::WorkRoute>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing_source_uid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing_inherited_from: Option<String>,
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
            parent_task_uid: None,
            source_branch: None,
            destination_branch: None,
            source_sha: None,
            parallelizable: None,
            routing_overrides: Default::default(),
            routing_source_uid: None,
            routing_inherited_from: None,
        };
        metadata.validate(Some(identity))?;
        Ok(metadata)
    }

    pub fn with_parent_task_uid(mut self, parent_task_uid: Option<&str>) -> anyhow::Result<Self> {
        self.parent_task_uid = parent_task_uid.map(str::to_owned);
        self.validate(None)?;
        Ok(self)
    }

    pub fn with_branch_targets(
        mut self,
        targets: Option<&crate::core::workflow::BranchTargets>,
    ) -> anyhow::Result<Self> {
        self.source_branch = targets.map(|targets| targets.source.clone());
        self.destination_branch = targets.map(|targets| targets.destination.clone());
        self.validate(None)?;
        Ok(self)
    }

    pub fn with_task_routing(
        mut self,
        routes: &std::collections::BTreeMap<
            String,
            crate::persistence::harness_settings::WorkRoute,
        >,
        source_uid: Option<&str>,
        inherited_from: Option<&str>,
    ) -> anyhow::Result<Self> {
        self.routing_overrides = routes.clone();
        self.routing_source_uid = source_uid.map(str::to_owned);
        self.routing_inherited_from = inherited_from.map(str::to_owned);
        self.validate(None)?;
        Ok(self)
    }

    pub fn with_source_sha(mut self, source_sha: Option<&str>) -> anyhow::Result<Self> {
        self.source_sha = source_sha.map(str::to_owned);
        self.validate(None)?;
        Ok(self)
    }

    pub fn with_parallelizable(mut self, parallelizable: Option<bool>) -> anyhow::Result<Self> {
        self.parallelizable = parallelizable;
        self.validate(None)?;
        Ok(self)
    }

    pub fn validate(
        &self,
        identity: Option<&crate::domain::ArtifactIdentity>,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            (1..=VERSION).contains(&self.schema_version),
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
        if let Some(parent_uid) = self.parent_task_uid.as_deref() {
            anyhow::ensure!(
                uuid::Uuid::parse_str(parent_uid).is_ok() && parent_uid != self.uid,
                "Task metadata has an invalid parent task identity"
            );
        }
        anyhow::ensure!(
            self.dependency_uids.len() <= MAX_DEPENDENCIES,
            "Task metadata has too many dependencies"
        );
        routing::validate_routing(&self.routing_overrides)?;
        for (label, uid) in [
            ("routing source", self.routing_source_uid.as_deref()),
            ("routing parent", self.routing_inherited_from.as_deref()),
        ] {
            if let Some(uid) = uid {
                anyhow::ensure!(
                    uuid::Uuid::parse_str(uid).is_ok(),
                    "Task metadata has an invalid {label} UID"
                );
            }
        }
        for branch in [
            self.source_branch.as_deref(),
            self.destination_branch.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            anyhow::ensure!(
                valid_branch(branch),
                "Task metadata has an invalid branch name"
            );
        }
        if let Some(sha) = self.source_sha.as_deref() {
            anyhow::ensure!(
                matches!(sha.len(), 40 | 64) && sha.bytes().all(|byte| byte.is_ascii_hexdigit()),
                "Task metadata has an invalid source SHA"
            );
        }
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

fn valid_branch(branch: &str) -> bool {
    !branch.is_empty()
        && branch.len() <= 255
        && !branch.starts_with('-')
        && !branch.ends_with('/')
        && !branch.contains("..")
        && !branch.contains("@{")
        && !branch
            .chars()
            .any(|ch| ch.is_whitespace() || "~^:?*[\\".contains(ch))
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
    let Some(json) = field.strip_prefix("koolade-task: ") else {
        return Ok(None);
    };
    anyhow::ensure!(
        lines.all(|line| line.trim().is_empty()),
        "Unexpected field in Koolade task front matter"
    );
    let metadata: TaskMetadata = serde_json::from_str(json)?;
    metadata.validate(None)?;
    Ok(Some(metadata))
}

pub fn embed(markdown: &str, metadata: &TaskMetadata) -> anyhow::Result<String> {
    metadata.validate(None)?;
    let body = frontmatter(markdown)?.map_or(markdown, |(_, body)| body);
    let value = serde_json::to_string(metadata)?;
    Ok(format!("---\nkoolade-task: {value}\n---\n\n{body}"))
}

pub fn visible_content(markdown: &str) -> String {
    let body = frontmatter(markdown)
        .ok()
        .flatten()
        .map_or(markdown, |(_, body)| body);
    crate::domain::ArtifactIdentity::visible_markdown(body)
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
