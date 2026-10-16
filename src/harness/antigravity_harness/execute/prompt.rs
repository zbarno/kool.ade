use crate::harness::PlanningRequest;

#[cfg(test)]
pub(in crate::harness) fn prompt_input(request: &PlanningRequest) -> String {
    prompt_input_with_policy(request, "")
}

pub(in crate::harness) fn prompt_input_with_policy(
    request: &PlanningRequest,
    boundary_policy: &str,
) -> String {
    let prompt = format!(
        "Kool.ad/e owns task lifecycle, source and destination branches, approvals, Git operations, and publication. Never switch branches, commit, push, open a pull request, or publish. Work only on the task requested below and return its required structured result.\n\n{}\n\n{}\n\n{}",
        request.system_instructions, boundary_policy, request.prompt_body
    );
    serde_json::json!({"event":"user","message":{"content":prompt}}).to_string() + "\n"
}
