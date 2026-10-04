use crate::harness::ExecutionMode;

pub(super) fn working_tree_fingerprint(repo: &std::path::Path) -> Option<u64> {
    let output = std::process::Command::new("git")
        .args([
            "diff",
            "--no-ext-diff",
            "--binary",
            "HEAD",
            "--",
            ".",
            ":(exclude).koolade-packet/state/work.json",
        ])
        .current_dir(repo)
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    Some(crate::persistence::fnv1a64(&output.stdout))
}

pub(super) fn previous_response_for_repair(feedback: &[String], previous: &str) -> String {
    if feedback.iter().any(|problem| {
        problem.contains("no complete JSON story")
            || problem.contains("No complete JSON object was found")
    }) {
        "Return ONLY one complete JSON object. No preface, explanation, status, Markdown fence, or second attempt. The first non-whitespace character must be { and the last non-whitespace character must be }. Include exactly one task_stories entry matching this outline task. Keep the object below 3,000 characters: use one short sentence in required text fields, one short string per required list, and empty arrays for optional lists. Do not include code snippets, quoted phrases, or backslashes inside string values. Do not mention the repair or your reasoning."
            .into()
    } else if feedback.iter().any(|problem| {
        problem.contains("Invalid task story JSON") || problem.contains("Invalid task story schema")
    }) {
        "The previous story was malformed. Discard it completely and rebuild from the supplied schema. Return ONLY one complete JSON object with exactly one task_stories entry matching this outline task. No preface, explanation, Markdown fence, or second attempt. Keep the object below 3,000 characters: one short sentence per required text field, one short string per required list, and empty arrays for optional lists. Use single-level arrays of strings. Do not include code snippets, quoted phrases, or backslashes inside string values. Do not mention the repair or your reasoning."
            .into()
    } else if feedback
        .iter()
        .any(|problem| problem.contains("read-only planning"))
    {
        "Repository inspection reached Kool.ad/e's bounded read limit. Do not call tools again; complete the response from the supplied approved context and evidence already gathered. If a detail is unknown, state it as a focused discovery step."
            .into()
    } else if feedback
        .iter()
        .any(|problem| problem.contains("Return exactly one complete story in task_stories"))
    {
        "The previous response did not contain exactly one story. Discard it completely. Return ONLY one complete JSON object with exactly one task_stories entry matching the current outline task. No preface, explanation, Markdown fence, or second attempt. Keep it below 2,500 characters with one short sentence per required text field, one short string per required list, and empty arrays for optional lists. Do not include code snippets, quoted phrases, backslashes, or another placeholder story. Do not mention your reasoning."
            .into()
    } else if feedback.iter().any(|problem| {
        problem.contains("Field budget overages") || problem.contains("complete story is")
    }) {
        "The previous story exceeded its size budgets. Discard it completely and rebuild the same single outline task. Return ONLY one complete JSON object with exactly one task_stories entry; no preface, explanation, Markdown fence, or second attempt. Keep the object below 2,500 characters: intent <=200; goal <=250; context <=250; affected_files <=250 total; implementation_steps <=500 total (at most 3 short strings); acceptance_criteria <=400 total (at most 2 short strings); test_plan <=300 total (at most 2 short strings); definition_of_done one string <=80. Use one short sentence each for user_story and purpose. Keep technical_design and edge_cases empty, rollout_notes empty, and include one concrete runnable verification command under 200 characters. Preserve only this task's approved behavior; omit speculative detail and work owned by other tasks. Do not repeat requirements. No code snippets, quoted phrases, or backslashes inside string values."
            .into()
    } else {
        previous.chars().take(60_000).collect()
    }
}

pub(super) fn repair_is_response_only(feedback: &[String]) -> bool {
    feedback.iter().any(|problem| {
        problem.contains("Field budget overages")
            || problem.contains("complete story is")
            || problem.contains("Invalid task story JSON")
            || problem.contains("No complete JSON object was found")
            || problem.contains("Return exactly one complete story in task_stories")
            || problem.contains("read-only planning")
    })
}

pub(super) fn repair_execution_mode(
    requested: ExecutionMode,
    feedback: &[String],
) -> ExecutionMode {
    if feedback.is_empty() || !repair_is_response_only(feedback) {
        requested
    } else {
        // A repair corrects an already gathered response. Enforce the
        // response-only contract through harness capabilities, not just
        // prompt wording, so retries cannot spend the read budget again.
        ExecutionMode::ReadOnlyAnalysis
    }
}
