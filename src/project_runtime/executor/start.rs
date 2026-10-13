use std::{collections::BTreeMap, time::Duration};

use crate::{
    execution::session::ExecutionSession,
    project_runtime::{
        ProjectCapabilityManifestV1, ProjectRuntimeStore, RecipeSource, RuntimeOperation,
        RuntimeReceiptState, RuntimeReceiptV1, TargetCapabilityState,
    },
};

const MAX_START_SECONDS: u64 = 120;
const READINESS_ATTEMPTS: usize = 40;

pub(super) fn execute(
    store: &ProjectRuntimeStore<'_>,
    session: &ExecutionSession,
    manifest: &ProjectCapabilityManifestV1,
    target: &super::super::AppTargetV1,
) -> anyhow::Result<Vec<RuntimeReceiptV1>> {
    anyhow::ensure!(
        target.capability_state == TargetCapabilityState::Ready,
        "runtime target is not approved as ready to start"
    );
    let recipe_id = target
        .start_recipe_ref
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("runtime target has no start recipe"))?;
    let probe_id = target
        .readiness_probe_ref
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("runtime target has no readiness probe"))?;
    let recipe = manifest
        .recipes
        .iter()
        .chain(manifest.user_overrides.iter())
        .find(|recipe| recipe.recipe_id.as_str() == recipe_id)
        .ok_or_else(|| anyhow::anyhow!("runtime start recipe is missing"))?;
    anyhow::ensure!(
        matches!(
            recipe.source,
            RecipeSource::UserApproved | RecipeSource::TrustedAdapter
        ),
        "project metadata recipes require explicit approval before execution"
    );
    let probe = manifest
        .readiness_probes
        .get(probe_id)
        .ok_or_else(|| anyhow::anyhow!("runtime readiness probe is missing"))?;
    probe
        .validate()
        .map_err(|_| anyhow::anyhow!("runtime readiness probe failed validation"))?;
    let script = start_script(manifest, target, recipe, probe)?;
    let timeout = Duration::from_secs(recipe.timeout_seconds.min(MAX_START_SECONDS));
    let (output, process) = match session.start_runtime_process(&script, timeout) {
        Ok(result) => result,
        Err(_) => {
            let receipt = receipt(session, target, recipe.recipe_id.clone(), None, None, false);
            store.append_receipt(&receipt)?;
            return Ok(vec![receipt]);
        }
    };
    let passed = process.is_some()
        && output.result.exit_code == Some(0)
        && !output.result.cancelled
        && !output.result.timed_out;
    let receipt = receipt(
        session,
        target,
        recipe.recipe_id.clone(),
        output.result.exit_code,
        Some(output.result.execution_id),
        passed,
    );
    if let Err(error) = store.append_receipt(&receipt) {
        if let Some(process) = process {
            let _ = process.stop();
        }
        return Err(error.into());
    }
    Ok(vec![receipt])
}

fn receipt(
    session: &ExecutionSession,
    target: &super::super::AppTargetV1,
    recipe_id: String,
    exit_code: Option<i32>,
    log_reference: Option<String>,
    passed: bool,
) -> RuntimeReceiptV1 {
    RuntimeReceiptV1 {
        schema_version: 1,
        receipt_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        operation: RuntimeOperation::Start,
        task_id: session.workspace().task_id.clone(),
        target_id: target.target_id.clone(),
        source_sha: session.workspace().source_sha.clone(),
        recipe_id: Some(recipe_id),
        state: if passed {
            RuntimeReceiptState::Passed
        } else {
            RuntimeReceiptState::Failed
        },
        exit_code,
        artifact_hashes: BTreeMap::new(),
        log_reference,
        observed_at_unix_ms: super::now_unix_ms(),
    }
}

fn start_script(
    manifest: &ProjectCapabilityManifestV1,
    target: &super::super::AppTargetV1,
    start: &crate::project_runtime::ExecutionRecipeV1,
    probe: &crate::project_runtime::ReadinessProbeV1,
) -> anyhow::Result<String> {
    let project_root = &target.project_relative_root;
    let start_directory =
        super::join_workspace_path(project_root, &start.working_directory_relative)?;
    let start_command = shell_argv(&start.executable_id, &start.argv);
    let readiness = readiness_command(manifest, target, probe)?;
    let launch = format!(
        "(cd {} && exec {})",
        shell_quote(&start_directory),
        start_command
    );
    Ok(format!(
        "set -u\ncd {} || exit 70\n{} >/dev/null 2>&1 &\npid=$!\ni=0\nwhile [ \"$i\" -lt {READINESS_ATTEMPTS} ]; do\n  if ! kill -0 \"$pid\" 2>/dev/null; then printf '%s\\n' 'start process exited before readiness' >&2; exit 71; fi\n  if {readiness}; then printf '%s\\n' 'readiness probe passed'; exit 0; fi\n  i=$((i + 1))\n  sleep 0.25\ndone\nprintf '%s\\n' 'readiness probe timed out' >&2\nexit 124\n",
        shell_quote(project_root),
        launch,
    ))
}

fn readiness_command(
    manifest: &ProjectCapabilityManifestV1,
    target: &super::super::AppTargetV1,
    probe: &crate::project_runtime::ReadinessProbeV1,
) -> anyhow::Result<String> {
    use crate::project_runtime::ReadinessProbeV1;
    match probe {
        ReadinessProbeV1::ProcessAlive { executable_id } => {
            let start_id = target
                .start_recipe_ref
                .as_deref()
                .and_then(|id| {
                    manifest
                        .recipes
                        .iter()
                        .chain(manifest.user_overrides.iter())
                        .find(|recipe| recipe.recipe_id == id)
                })
                .map(|recipe| recipe.executable_id.as_str());
            anyhow::ensure!(
                start_id == Some(executable_id.as_str()),
                "process readiness probe must identify the approved start executable"
            );
            Ok("kill -0 \"$pid\" 2>/dev/null".into())
        }
        ReadinessProbeV1::HttpOnApprovedPort {
            port,
            path,
            expected_status,
        } => http_readiness_command(manifest, target, *port, path, Some(*expected_status)),
        ReadinessProbeV1::LocalServiceHealth { port, path } => {
            http_readiness_command(manifest, target, *port, path, None)
        }
        ReadinessProbeV1::CustomApprovedProbe { recipe_id } => {
            let recipe = manifest
                .recipes
                .iter()
                .chain(manifest.user_overrides.iter())
                .find(|recipe| recipe.recipe_id.as_str() == recipe_id.as_str())
                .ok_or_else(|| anyhow::anyhow!("custom readiness recipe is missing"))?;
            anyhow::ensure!(
                matches!(
                    recipe.source,
                    RecipeSource::UserApproved | RecipeSource::TrustedAdapter
                ),
                "custom readiness recipe requires explicit approval"
            );
            let directory = super::join_workspace_path(
                &target.project_relative_root,
                &recipe.working_directory_relative,
            )?;
            Ok(format!(
                "(cd {} && {}) >/dev/null 2>&1",
                shell_quote(&directory),
                shell_argv(&recipe.executable_id, &recipe.argv)
            ))
        }
        ReadinessProbeV1::WindowVisible { .. } => {
            anyhow::bail!("window readiness requires an approved desktop automation driver")
        }
    }
}

/// Builds an application-owned HTTP probe from a standard-library client in the
/// already selected executor image. No project script or network tool is used.
fn http_readiness_command(
    manifest: &ProjectCapabilityManifestV1,
    target: &super::super::AppTargetV1,
    port: u16,
    path: &str,
    expected_status: Option<u16>,
) -> anyhow::Result<String> {
    let start_id = target
        .start_recipe_ref
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("HTTP readiness requires an approved start recipe"))?;
    let start = manifest
        .recipes
        .iter()
        .chain(manifest.user_overrides.iter())
        .find(|recipe| recipe.recipe_id == start_id)
        .ok_or_else(|| anyhow::anyhow!("HTTP readiness start recipe is missing"))?;
    let expected = expected_status.map_or_else(
        || "200 <= status < 400".to_owned(),
        |code| format!("status == {code}"),
    );
    if start
        .prerequisite_tool_ids
        .iter()
        .any(|tool| tool == "node")
    {
        let path = serde_json::to_string(path)?;
        let script = format!(
            "const http=require('node:http');const r=http.get({{host:'127.0.0.1',port:{port},path:{path},timeout:1500}},s=>{{s.resume();process.exit({})}});r.on('timeout',()=>{{r.destroy();process.exit(1)}});r.on('error',()=>process.exit(1));",
            if expected_status.is_some() {
                format!(
                    "s.statusCode === {} ? 0 : 1",
                    expected_status.unwrap_or_default()
                )
            } else {
                "s.statusCode >= 200 && s.statusCode < 400 ? 0 : 1".into()
            }
        );
        return Ok(shell_argv("node", &["-e".into(), script]));
    }
    if start
        .prerequisite_tool_ids
        .iter()
        .any(|tool| tool == "python3")
    {
        let path = serde_json::to_string(path)?;
        let script = format!(
            "import http.client,sys; c=http.client.HTTPConnection('127.0.0.1',{port},timeout=1.5); c.request('GET',{path}); s=c.getresponse().status; sys.exit(0 if {expected} else 1)"
        );
        return Ok(shell_argv("python3", &["-c".into(), script]));
    }
    anyhow::bail!(
        "HTTP readiness requires the approved start recipe to declare the node or python3 probe prerequisite"
    )
}

fn shell_argv(executable_id: &str, argv: &[String]) -> String {
    std::iter::once(executable_id)
        .chain(argv.iter().map(String::as_str))
        .map(shell_quote)
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
#[path = "start/tests.rs"]
mod tests;
