use crate::project_runtime::{ExecutionRecipeV1, ReadinessProbeV1};

const RUNNER_SOURCE: &str = include_str!("go_runner.go");

pub(super) fn start_script(
    start: &ExecutionRecipeV1,
    probe: &ReadinessProbeV1,
) -> anyhow::Result<String> {
    build_command(start, probe)
}

pub(super) fn build_command(
    start: &ExecutionRecipeV1,
    probe: &ReadinessProbeV1,
) -> anyhow::Result<String> {
    let (port, path, expected_status) = match probe {
        ReadinessProbeV1::HttpOnApprovedPort {
            port,
            path,
            expected_status,
        } => (*port, path.as_str(), expected_status.to_string()),
        ReadinessProbeV1::LocalServiceHealth { port, path } => {
            (*port, path.as_str(), "0".to_owned())
        }
        _ => anyhow::bail!("Go readiness requires a supported HTTP probe"),
    };
    let mut arguments = vec!["\"$probe_dir\"".to_owned()];
    arguments.extend(
        [
            start.working_directory_relative.clone(),
            start.executable_id.clone(),
            port.to_string(),
            path.to_owned(),
            expected_status,
        ]
        .into_iter()
        .chain(start.argv.iter().cloned())
        .map(|argument| shell_quote(&argument)),
    );
    let arguments = arguments.join(" ");
    Ok(format!(
        "set -u\ncd '.' || exit 70\nprobe_dir='/workspace/.koolade-runtime-qa-'$$\nmkdir \"$probe_dir\" || exit 72\nprintf '%s' {} > \"$probe_dir/runner.go\" || exit 72\nif ! (cd /tmp && GO111MODULE=off GOTOOLCHAIN=local GOPROXY=off GOSUMDB=off GOTMPDIR=/workspace 'go' 'build' '-trimpath' '-o' \"$probe_dir/runner\" \"$probe_dir/runner.go\"); then printf '%s\\n' 'approved Go readiness runner build failed' >&2; exit 72; fi\nexec \"$probe_dir/runner\" {arguments}\n",
        shell_quote(RUNNER_SOURCE),
    ))
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
