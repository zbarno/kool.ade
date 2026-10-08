use super::*;

pub(in crate::core::implementation::tests) fn save_legacy_reconciliation_state(
    sandbox: &Sandbox,
    ticket: &str,
    worktree: &Path,
    base: &str,
    local: &str,
    remote: &str,
    common: &str,
) -> std::path::PathBuf {
    let dir = state_dir(&sandbox.repo, ticket).unwrap();
    fs::create_dir_all(&dir).unwrap();
    crate::core::implementation::initial_reconciliation::save_plan(
        &dir,
        base,
        local,
        remote,
        common,
        &[],
    )
    .unwrap();
    let task_key = crate::core::implementation::key_for_ticket(ticket);
    for (side, commit) in [("local", local), ("remote", remote)] {
        sandbox.git(
            &sandbox.repo,
            &[
                "update-ref",
                &format!("refs/koolade-reconciliations/{task_key}/{side}"),
                commit,
            ],
        );
    }
    let state: Implementation = serde_json::from_value(serde_json::json!({
        "ticket": ticket,
        "ticket_text": fs::read_to_string(sandbox.repo.join(ticket)).unwrap(),
        "branch": format!("koolade/{task_key}"),
        "source_branch": null,
        "destination_branch": null,
        "base": base,
        "base_commit": remote,
        "task_repository": worktree,
        "task_repository_kind": "legacy_worktree",
        "task_repository_ready": true,
        "task_repositories": [worktree],
        "status": "preparing",
        "detail": ""
    }))
    .unwrap();
    save(&dir, &state).unwrap();
    dir
}

pub(in crate::core::implementation::tests) fn complete_report(command: &str) -> serde_json::Value {
    let criteria = crate::core::implementation::initial_reconciliation::CONTRACT
        .lines()
        .filter_map(|line| line.strip_prefix("- "))
        .map(|criterion| serde_json::json!({"criterion":criterion,"evidence":"Both histories were checked and the combined baseline verification passed."}))
        .collect::<Vec<_>>();
    serde_json::json!({
        "schemaVersion":2,
        "status":"complete",
        "blocker_disposition":"none",
        "summary":"Both starting histories are integrated and the combined baseline passed its checks.",
        "acceptance_criteria":criteria,
        "verification":[command],
        "remaining":[],
        "human_choices":[]
    })
}

pub(in crate::core::implementation::tests) fn outcome(
    value: serde_json::Value,
) -> crate::harness::HarnessOutcome {
    crate::harness::HarnessOutcome {
        final_text: value.to_string(),
        envelope: None,
        stderr_tail: String::new(),
    }
}
