use super::*;

pub(super) struct Context<'a> {
    pub(super) state: &'a Implementation,
    pub(super) status: &'a str,
    pub(super) log: &'a str,
    pub(super) specification: &'a str,
    pub(super) history_path: &'a Path,
    pub(super) history_evidence: &'a str,
    pub(super) user_context: Option<&'a str>,
    pub(super) prior_failure: &'a str,
    pub(super) feedback: &'a str,
    pub(super) previous_response: &'a str,
    pub(super) dir: &'a Path,
    pub(super) stamp: i64,
}

pub(super) fn build(c: Context<'_>) -> (String, PathBuf) {
    let mut prompt = format!(
        "Implement this ticket in the CURRENT working directory, a dedicated Git worktree. This may be a RESUME: inspect git status, existing diffs, commits, untracked files, tests and repository instructions FIRST. Preserve and complete existing work; do not restart, reset, clean, discard or overwrite unrelated changes. Verify prerequisites and dependencies; report blocked if unavailable. Implement only this ticket's scope. Run the required checks and repair failures. Do not change branches, create worktrees, commit, push, create PRs or merge; Kool.ad/e owns those steps. Do not modify the original checkout.\n\nTICKET PATH: {}\nTICKET CONTENT:\n{}\n\nAPPROVED SPECIFICATION:\n{}\n\nAFFECTED PRODUCT MODULES (FROZEN AT TASK APPROVAL):\n{}\n\nCURRENT STATUS:\n{}\nRECENT COMMITS:\n{}\n\nReturn a complete JSON object with status (complete or blocked), summary, acceptance_criteria (array of objects with criterion copied verbatim from the ticket and concrete evidence), verification (array of runnable POSIX /bin/sh commands; each runs in a NEW shell starting in this worktree, with KOOLADE_WORKTREE set to its absolute path; no shell variables or cwd changes carry between commands), remaining (array of unresolved work). Complete requires every ticket criterion met, meaningful checks passing, and remaining empty. Use actual commands without placeholder paths. Before changing directories, capture paths or use \"$KOOLADE_WORKTREE/Cargo.toml\"; $(pwd) after cd refers to the NEW directory. Do not use Bash-only syntax. When testing commands yourself, export KOOLADE_WORKTREE to this worktree path before invoking /bin/sh. Execute exactly the commands you report using /bin/sh. Assert expected outcomes and preserve command exit failures: capture output to a file, then check it, rather than masking a failed command with a successful pipeline or command substitution. Never claim success from an exit code alone or invent results. Do not include prose outside the JSON.",
        c.state.ticket,
        c.state.ticket_text,
        c.specification,
        c.state
            .approved_product_context
            .as_deref()
            .unwrap_or("Legacy task: no scoped product snapshot."),
        c.status,
        c.log
    );
    prompt.push_str(report::response_contract());
    prompt.insert_str(0, report::feasibility_preflight());
    prompt.push_str(&format!(
        "\n\nMECHANICALLY COLLECTED HISTORY PREFLIGHT (also saved at {}):\n{}\nCompare any ticket-stated exact footprint with these reachable-history facts before editing. Explicitly say whether the expected table describes cumulative feature history or this ticket's changes from its task base.\n",
        c.history_path.display(), c.history_evidence
    ));
    prompt.push_str("Write summary for an operator in at most 400 characters: state the outcome and why work is paused, without test inventories or repeated evidence. Keep detailed proof in acceptance_criteria, verification, and the saved report. Make each remaining entry start with the responsible person or role and a verb; name the artifact and result briefly. Use human_choices for actual alternatives instead of embedding a fixed lettered list in prose. Separate human steps from Kool.ad/e's follow-up.\n");
    if let Some(input) = c.user_context.filter(|input| !input.trim().is_empty()) {
        prompt.push_str(&format!("\n\nLATEST SUBMITTED USER RESPONSE FOR THIS TASK:\n{}\nThis is a user-supplied decision or observation, not proof that the ledger was changed or external checks were run. Apply only what it explicitly authorizes; verify any required decision-maker identity, inspect the relevant artifacts, and keep unmet requirements blocked.\n",
            crate::core::context_build::clip(input, 4000)));
    }
    if let Some(dependencies) = &c.state.completed_dependency_context {
        prompt.push_str(&format!(
            "\n\nCOMPLETED DEPENDENCY CONTRACTS:\n{dependencies}"
        ));
    }
    if !c.prior_failure.is_empty() {
        prompt.push_str(&format!("\n\nPREVIOUS STOP / CORRECTION REQUIRED (prior run, context only):\n{}\nThis run has a fresh report, verification, harness, and self-repair budget. Prior attempts do not consume it. Preserve previous work; do not treat previous retry exhaustion as a current blocker. Actual unmet prerequisites and acceptance checks still apply.\n", c.prior_failure));
    }
    if !c.feedback.is_empty() {
        prompt.push_str(&format!("\n\nPREVIOUS STOP / CORRECTION REQUIRED:\n{}\nContinue in this same worktree. Treat this as a correction history: keep earlier fixes and address the newest failure without reintroducing older ones. Inspect and preserve existing work. Correct the report or implementation and rerun affected checks. Copy acceptance criterion text EXACTLY, including any spelling mistakes; do not edit the ticket to satisfy this check. Return the full JSON report, not just the correction. Do not weaken or bypass failing checks. Before repeating recovery, check whether the failure is a fixed contradiction in the frozen base/history; if so, preserve the evidence and report the exact human decision needed instead of repeating machine checks. Report blocked for prerequisites or decisions that require human intervention.\nPrevious response (possibly truncated):\n{}", c.feedback, c.previous_response));
    }
    let report_path = c.dir.join(format!("{}-report.json", c.stamp));
    prompt.push_str("\n\nAfter verification, return the complete JSON report as your final response. Kool.ad/e saves that report outside the sandbox. Do not write a report file with koolade_bash.\nYou may fix the root cause of encountered failures and add regression coverage in this worktree when necessary. Keep repairs focused, preserve checks, and do not commit them yourself: Kool.ad/e verifies and commits the task and its recovery fixes together atomically.\n");
    (prompt, report_path)
}
