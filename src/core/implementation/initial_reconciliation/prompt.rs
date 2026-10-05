use super::Plan;

pub(super) fn build(
    plan: &Plan,
    status: &str,
    unmerged: &[String],
    diff: &str,
    user_context: Option<&str>,
    feedback: &str,
    previous_response: &str,
) -> String {
    let mut prompt = format!(
        "INITIAL BASE RECONCILIATION\n\nThe local development history and the fetched shared history diverged before this implementation started. The application created this isolated worktree from the shared commit and began merging the local commit. The original checkout has not been changed.\n\nLOCAL COMMIT: {}\nSHARED COMMIT: {}\nCOMMON ANCESTOR: {}\n\nRead this repository's AGENTS.md and documented quality guidance. Inspect both histories and the combined diff. Resolve conflicts while preserving intended behavior from both sides. Do not implement the later task. Only modify paths already changed by one or both pinned commits. Do not add build configuration, helper files, or other repository changes to work around a sandbox verification failure. The application captured required quality commands from applicable AGENTS.md files and will run them after your report; do not run those baseline commands yourself. If one cannot run because the sandbox lacks a tool, package feed, cache, or memory, report environment_prerequisite and stop. Before your edits, the application has captured explicit required quality commands from applicable AGENTS.md files in both pinned histories. It will execute those commands and any additional commands in the reconciled AGENTS.md, even if your report omits them. It will verify both commits are ancestors of the result and create the merge commit.\n\nCURRENT WORKTREE STATUS:\n{}\nUNMERGED PATHS:\n{}\nCOMBINED DIFF:\n{}\n\nReturn the same strict JSON report used by an implementation task. Copy these acceptance criteria exactly: 1) Local and fetched shared changes are both preserved in the reconciled starting point. 2) Repository-required baseline checks pass on the combined result. Give concrete evidence for both criteria and runnable POSIX /bin/sh verification commands drawn from repository instructions. The application will execute those commands in a separate sandbox. Do not claim that checks pass unless you have run them successfully yourself. If the combined behavior cannot be decided safely from repository evidence, report blocked with blocker_disposition human_action. If a required tool, dependency, cache, or network resource is unavailable to the sandbox and cannot be provisioned by the worktree, report blocked with blocker_disposition environment_prerequisite and state exactly what environment resource is missing. Never report an unavailable environment resource as a machine repair, and do not spend correction attempts repeating work that the sandbox cannot perform.\n",
        plan.local_commit,
        plan.remote_commit,
        plan.common_base,
        clipped(status, 5000),
        if unmerged.is_empty() {
            "(none)".into()
        } else {
            unmerged.join("\n")
        },
        clipped(diff, 12000)
    );
    if let Some(input) = user_context.filter(|text| !text.trim().is_empty()) {
        prompt.push_str(&format!(
            "\nLATEST USER RESPONSE FOR THIS TASK:\n{}\nApply only the decision it explicitly authorizes; it is not proof that reconciliation or checks have succeeded.\n",
            clipped(input, 4000)
        ));
    }
    if !feedback.is_empty() {
        prompt.push_str(&format!(
            "\nPREVIOUS STOP / CORRECTION REQUIRED:\n{}\nContinue in this same worktree. Preserve all existing work, repair the reported issue, rerun affected checks, and return the complete JSON report.\nPrevious response:\n{}\n",
            clipped(feedback, 5000),
            clipped(previous_response, 5000)
        ));
    }
    prompt.push_str(
        "\nReturn only the complete JSON object with schemaVersion 2, status, blocker_disposition, summary, acceptance_criteria, verification, remaining, and human_choices. Do not write a report file. Do not stage or commit. The application owns Git metadata.\n",
    );
    prompt
}

fn clipped(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        text.to_owned()
    } else {
        let mut end = limit;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}\n[truncated]", &text[..end])
    }
}
