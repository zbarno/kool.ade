use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionPublicationStatus {
    #[default]
    Planned,
    Running,
    Verified,
    Shared,
    Merged,
    ClosedUnmerged,
    Failed,
}

impl ExecutionPublicationStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Planned => "Planned",
            Self::Running => "Running",
            Self::Verified => "Verified",
            Self::Shared => "Shared for review",
            Self::Merged => "Merged",
            Self::ClosedUnmerged => "Closed without merge",
            Self::Failed => "Needs attention",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PullRequestLinkage {
    pub repository_id: String,
    pub number: u64,
    pub url: String,
    pub head_sha: String,
    pub base_branch: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_sha: Option<String>,
    pub status: ExecutionPublicationStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TaskExecutionLinkage {
    pub repository_id: String,
    pub source_branch: Option<String>,
    pub source_sha: Option<String>,
    pub destination_branch: Option<String>,
    pub implementation_branch: Option<String>,
    pub publication_status: ExecutionPublicationStatus,
    pub pull_requests: Vec<PullRequestLinkage>,
}

pub(super) fn validate(
    repository_id: &str,
    source_branch: Option<&str>,
    source_sha: Option<&str>,
    destination_branch: Option<&str>,
    implementation_branch: Option<&str>,
    pull_requests: &[PullRequestLinkage],
    path: &str,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        !repository_id.trim().is_empty()
            && repository_id.len() <= 128
            && !repository_id.chars().any(char::is_control),
        "Task state {path} has an invalid repository ID"
    );
    for branch in [source_branch, destination_branch, implementation_branch]
        .into_iter()
        .flatten()
    {
        anyhow::ensure!(
            valid_branch(branch),
            "Task state {path} has an invalid branch"
        );
    }
    if let Some(sha) = source_sha {
        anyhow::ensure!(
            valid_sha(sha),
            "Task state {path} has an invalid source SHA"
        );
    }
    let mut seen = std::collections::BTreeSet::new();
    for pull_request in pull_requests {
        anyhow::ensure!(
            !pull_request.repository_id.trim().is_empty()
                && pull_request.repository_id.len() <= 128
                && pull_request.number > 0
                && pull_request.url.starts_with("https://")
                && pull_request.url.len() <= 2048
                && !pull_request.url.chars().any(char::is_control)
                && valid_sha(&pull_request.head_sha)
                && valid_branch(&pull_request.base_branch)
                && pull_request.merge_sha.as_deref().is_none_or(valid_sha),
            "Task state {path} has invalid pull request linkage"
        );
        anyhow::ensure!(
            seen.insert((pull_request.repository_id.as_str(), pull_request.number)),
            "Task state {path} contains a duplicate pull request"
        );
    }
    Ok(())
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

fn valid_sha(sha: &str) -> bool {
    matches!(sha.len(), 40 | 64) && sha.bytes().all(|byte| byte.is_ascii_hexdigit())
}
