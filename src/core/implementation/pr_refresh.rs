use super::*;
use crate::artifacts::planning_store::PlanningStore;

/// Refresh in a worker: GitHub outages must not block the UI or erase the
/// last confirmed state. The implementation lock prevents stale writes.
pub struct PrRefresh {
    rx: Receiver<Vec<(String, String)>>,
    cancel: Arc<AtomicBool>,
}
impl PrRefresh {
    pub fn start(repo: PathBuf, tickets: Vec<String>) -> Self {
        let store = PlanningStore::legacy_embedded(uuid::Uuid::nil(), &repo);
        Self::start_with_store(store, repo, tickets)
    }

    pub fn start_with_store(
        planning_store: PlanningStore,
        state_root: PathBuf,
        tickets: Vec<String>,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        std::thread::spawn(move || {
            let (progress, _updates) = mpsc::channel();
            let runner = Runner {
                gh: "gh".into(),
                runtime_config_source: None,
                deadline: Instant::now() + Duration::from_secs(45),
                cancel: worker_cancel,
                progress,

                authorization_verifier: None,
            };
            let mut errors = Vec::new();
            for ticket in tickets {
                if runner.remaining().is_err() {
                    break;
                }
                if let Err(error) =
                    refresh_pr_with_store(&planning_store, &state_root, &ticket, &runner)
                {
                    errors.push((ticket, format!("{error:#}")));
                }
            }
            let _ = tx.send(errors);
        });
        Self { rx, cancel }
    }
    pub fn poll(&self) -> Option<Vec<(String, String)>> {
        match self.rx.try_recv() {
            Ok(errors) => Some(errors),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(vec![(
                String::new(),
                "Task maintenance worker stopped unexpectedly; retrying on the next refresh".into(),
            )]),
        }
    }
}
impl Drop for PrRefresh {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}

#[cfg(test)]
pub(super) fn refresh_pr(repo: &Path, ticket: &str, runner: &Runner) -> anyhow::Result<()> {
    let store = PlanningStore::legacy_embedded(uuid::Uuid::nil(), repo);
    refresh_pr_with_store(&store, repo, ticket, runner)
}

fn refresh_pr_with_store(
    planning_store: &PlanningStore,
    state_root: &Path,
    ticket: &str,
    runner: &Runner,
) -> anyhow::Result<()> {
    let migration_gate = crate::artifacts::migration::acquire_project_state_gate(state_root)?;
    let task_uid = ticket_identity(planning_store, ticket)?;
    let dir = state_dir_for_task(state_root, ticket, task_uid.as_deref())?;
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join("run.lock"))?;
    if lock.try_lock().is_err() {
        // A concurrent implementation can hold this lock for minutes, so never
        // block. Brief platform stalls have, however, been observed to stretch
        // short critical sections past a single immediate attempt; give the
        // holder a bounded moment to finish before falling back to the
        // deliberate no-op.
        for delay_ms in [10u64, 20, 20, 20] {
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            if lock.try_lock().is_ok() {
                break;
            }
        }
        if lock.try_lock().is_err() {
            return Ok(());
        }
    }
    drop(migration_gate);
    let mut state = read_state_file(&dir.join("state.json"))?;
    let target_repo = target_repository_with_store(planning_store, state_root, ticket)?;
    if state.status == ImplementationStatus::Completed
        && (state.merged_commit.is_some() || state.pr_url.is_none())
    {
        if state.pr_state.is_some() {
            publication::update_pull_request_detail(&mut state);
        }
        cleanup::run(&target_repo, &dir, &mut state, runner);
        save(&dir, &state)?;
        return publication::reconcile_lifecycle_pr_state(planning_store, &state, &target_repo);
    }
    let Some(url) = state.pr_url.clone() else {
        return Ok(());
    };
    if state.pr_state == Some(PullRequestState::Merged) && state.merged_commit.is_some() {
        publication::update_pull_request_detail(&mut state);
        return save(&dir, &state);
    }
    state.pr_check_attempted_at = Some(chrono::Utc::now().to_rfc3339());
    let result = (|| -> anyhow::Result<(
        PullRequestState,
        Option<String>,
        Option<super::PullRequestDetails>,
    )> {
        let output = runner.command(
            &target_repo,
            &runner.gh,
            &[
                "pr",
                "view",
                &url,
                "--json",
                "state,mergeCommit,number,headRefOid,baseRefOid,baseRefName",
            ],
        )?;
        let value: serde_json::Value = serde_json::from_str(&output)?;
        let status = value["state"]
            .as_str()
            .and_then(PullRequestState::parse_api)
            .ok_or_else(|| anyhow::anyhow!("GitHub returned an unknown PR state"))?;
        let merged = value["mergeCommit"]["oid"]
            .as_str()
            .filter(|oid| !oid.is_empty())
            .map(str::to_owned);
        let details = match (
            value["number"].as_u64(),
            value["headRefOid"].as_str(),
            value["baseRefOid"].as_str(),
            value["baseRefName"].as_str(),
        ) {
            (Some(number), Some(head_sha), base_sha, Some(base_branch))
                if number > 0 && valid_sha(head_sha) && !base_branch.is_empty() =>
            {
                Some(super::PullRequestDetails {
                    number,
                    head_sha: head_sha.to_owned(),
                    base_sha: base_sha.filter(|sha| valid_sha(sha)).map(str::to_owned),
                    base_branch: base_branch.to_owned(),
                })
            }
            _ => None,
        };
        Ok((status, merged, details))
    })();
    match result {
        Ok((pull_request, merged, details)) => {
            state.status = match pull_request {
                PullRequestState::Merged => ImplementationStatus::Completed,
                PullRequestState::Closed => ImplementationStatus::PullRequestClosed,
                PullRequestState::Open => ImplementationStatus::AwaitingReview,
            };
            state.pr_state = Some(pull_request);
            if merged.is_some() {
                state.merged_commit = merged;
            }
            if details.is_some() {
                state.pr_details = details;
            }
            state.pr_checked_at = Some(chrono::Utc::now().to_rfc3339());
            state.pr_check_error = None;
            publication::update_pull_request_detail(&mut state);
        }
        Err(error) => state.pr_check_error = Some(error.to_string()),
    }
    cleanup::run(&target_repo, &dir, &mut state, runner);
    save(&dir, &state)?;
    publication::reconcile_lifecycle_pr_state(planning_store, &state, &target_repo)
}

fn valid_sha(sha: &str) -> bool {
    matches!(sha.len(), 40 | 64) && sha.bytes().all(|byte| byte.is_ascii_hexdigit())
}
