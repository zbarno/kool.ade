use super::super::ApplicationBoundary;

pub(super) fn configure(
    boundary: &ApplicationBoundary,
    argv: &mut Vec<String>,
    env: &mut Vec<(String, String)>,
) -> anyhow::Result<()> {
    let Some(server_args) = boundary.server_args()? else {
        argv.push("--no-tools".into());
        return Ok(());
    };

    let tools = boundary.mcp_tool_names();
    anyhow::ensure!(!tools.is_empty(), "Pi MCP session has no approved tools");
    let extension = boundary.scratch.write_file(
        "pi-koolade-mcp.ts",
        include_bytes!("../pi_mcp_extension.ts"),
    )?;
    env.extend([
        ("KOOLADE_MCP_COMMAND".into(), server_args[0].clone()),
        (
            "KOOLADE_MCP_ARGS".into(),
            serde_json::to_string(&server_args[1..])?,
        ),
        ("KOOLADE_MCP_TOOLS".into(), serde_json::to_string(&tools)?),
    ]);
    argv.extend([
        "--tools".into(),
        tools.join(","),
        "--extension".into(),
        extension.to_string_lossy().into_owned(),
    ]);
    if boundary.allows_curated_worker_skill() {
        let skill = boundary
            .scratch
            .create_directory("worker-skills/koolade-task")?;
        let skill_file = skill.join("SKILL.md");
        std::fs::write(&skill_file, include_bytes!("../pi_worker_skill.md"))?;
        argv.extend(["--skill".into(), skill_file.to_string_lossy().into_owned()]);
    }
    Ok(())
}
