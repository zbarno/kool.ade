use super::*;

impl Drop for Sandbox {
    fn drop(&mut self) {
        if let Ok(repo) = self.repo.canonicalize() {
            let project_id = crate::persistence::project_slug(&repo);
            let _ = fs::remove_dir_all(crate::persistence::project_dir(&project_id));
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

pub(crate) fn completed_cleanup_fixture(sandbox: &Sandbox) -> Implementation {
    let mut state = sandbox
        .run("complete", Arc::new(AtomicUsize::new(0)))
        .unwrap();
    let head = state.verified_head.as_deref().unwrap();
    sandbox.git(
        &state.task_repository,
        &[
            "push",
            sandbox.root.join("remote.git").to_str().unwrap(),
            &format!("{head}:refs/heads/main"),
        ],
    );
    state.status = ImplementationStatus::Completed;
    state.pr_state = Some(PullRequestState::Merged);
    state.merged_commit = state.verified_head.clone();
    save(&state_dir(&sandbox.repo, &sandbox.ticket).unwrap(), &state).unwrap();
    state
}
