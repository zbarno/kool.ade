use crate::harness::PlanningRequest;

pub(crate) fn command(
    binary: &std::path::Path,
    request: &PlanningRequest,
    model: Option<&str>,
    usage_path: Option<&std::path::Path>,
) -> Vec<String> {
    let mut args = vec![
        binary.to_string_lossy().into_owned(),
        "--silent".into(),
        "--no-ask-user".into(),
        "--no-auto-update".into(),
    ];
    if let Some(model) = model.filter(|model| !model.trim().is_empty()) {
        args.extend(["--model".into(), model.to_owned()]);
    }
    let access = request.mode.tool_access();
    let tools = match access {
        crate::harness::ToolAccess::None => "",
        crate::harness::ToolAccess::ReadOnly => "read",
        crate::harness::ToolAccess::BoundedImplementation => "read,edit",
    };
    args.push(format!("--available-tools={tools}"));
    if access == crate::harness::ToolAccess::BoundedImplementation {
        args.extend([
            "--allow-tool=write".into(),
            "--deny-tool=shell(git:*)".into(),
            "--deny-tool=shell(gh:*)".into(),
        ]);
    }
    args.push("--disable-builtin-mcps".into());
    if let Some(usage_path) = usage_path {
        args.extend([
            "--usage-output-file".into(),
            usage_path.to_string_lossy().into_owned(),
        ]);
    }
    let secrets = std::env::vars_os()
        .filter_map(|(name, _)| {
            let name = name.to_string_lossy();
            let upper = name.to_ascii_uppercase();
            ["TOKEN", "KEY", "SECRET", "PASSWORD", "CREDENTIAL", "AUTH"]
                .iter()
                .any(|needle| upper.contains(needle))
                .then(|| name.into_owned())
        })
        .collect::<Vec<_>>();
    if !secrets.is_empty() {
        args.push(format!("--secret-env-vars={}", secrets.join(",")));
    }
    args
}

pub(in crate::harness) fn prompt_input(request: &PlanningRequest, boundary_policy: &str) -> String {
    format!(
        "Kool.ad/e owns task lifecycle, branches, approvals, Git operations, and pull request publication. Never switch branches, commit, push, open a pull request, or publish. Work only inside the current task repository and return the requested structured result.\n\n{}\n\n{}\n\n{}",
        request.system_instructions, boundary_policy, request.prompt_body
    )
}
