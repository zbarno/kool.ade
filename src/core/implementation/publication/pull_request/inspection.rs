use super::*;

pub(super) struct PullRequestInspection {
    pub(super) details: super::super::super::PullRequestDetails,
    pub(super) state: PullRequestState,
}

pub(super) fn inspect_pull_request(
    url: &str,
    repository: &str,
    state: &Implementation,
    runner: &Runner,
) -> anyhow::Result<PullRequestInspection> {
    let response = runner.command(
        &state.task_repository,
        &runner.gh,
        &[
            "pr",
            "view",
            url,
            "--repo",
            repository,
            "--json",
            "url,state,number,headRefOid,baseRefOid,baseRefName,mergedAt",
        ],
    )?;
    let value: serde_json::Value = serde_json::from_str(&response)?;
    let provider_state = value["state"]
        .as_str()
        .and_then(PullRequestState::parse_api)
        .ok_or_else(|| anyhow::anyhow!("Provider omitted a valid pull request state"))?;
    anyhow::ensure!(
        matches!(
            provider_state,
            PullRequestState::Open | PullRequestState::Closed
        ),
        "Provider reports a merged pull request without an authoritative merge commit"
    );
    let details = pull_request_details(&value, Some(url)).ok_or_else(|| {
        anyhow::anyhow!("Provider omitted required pull request head or base details")
    })?;
    let reviewed_head = state
        .verified_head
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Verified task head is missing; refusing to record PR"))?;
    anyhow::ensure!(
        details.head_sha == reviewed_head,
        "Provider pull request head differs from the reviewed implementation head; publication needs attention and review evidence must be refreshed"
    );
    anyhow::ensure!(
        details.base_branch == state.base
            && details.base_sha.as_deref() == Some(state.base_commit.as_str()),
        "Provider pull request base differs from the reviewed destination; refresh integration, review, QA, and specification evidence before publication"
    );
    Ok(PullRequestInspection {
        details,
        state: provider_state,
    })
}

fn pull_request_details(
    value: &serde_json::Value,
    url: Option<&str>,
) -> Option<super::super::super::PullRequestDetails> {
    let url = url?;
    let number = value["number"]
        .as_u64()
        .or_else(|| pull_request_number(url))?;
    let head_sha = value["headRefOid"].as_str()?;
    let base_sha = value["baseRefOid"]
        .as_str()
        .filter(|sha| valid_sha(sha))
        .map(str::to_owned);
    let base_branch = value["baseRefName"].as_str()?;
    (number > 0 && valid_sha(head_sha) && !base_branch.is_empty()).then(|| {
        super::super::super::PullRequestDetails {
            number,
            head_sha: head_sha.to_owned(),
            base_sha,
            base_branch: base_branch.to_owned(),
            merged_at: value["mergedAt"].as_str().map(str::to_owned),
        }
    })
}

fn pull_request_number(url: &str) -> Option<u64> {
    let (_, path) = url.strip_prefix("https://")?.split_once('/')?;
    let mut segments = path.trim_end_matches('/').rsplit('/');
    let number = segments.next()?.parse::<u64>().ok()?;
    (segments.next()? == "pull" && number > 0).then_some(number)
}

fn valid_sha(sha: &str) -> bool {
    matches!(sha.len(), 40 | 64) && sha.bytes().all(|byte| byte.is_ascii_hexdigit())
}
