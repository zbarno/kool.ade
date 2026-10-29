use super::*;

#[test]
fn custom_readiness_uses_only_a_declared_approved_recipe_in_the_workspace() {
    let mut manifest = manifest();
    let mut probe_recipe = recipe();
    probe_recipe.recipe_id = "probe-web".into();
    probe_recipe.working_directory_relative = "tools".into();
    probe_recipe.executable_id = "python3".into();
    probe_recipe.argv = vec!["runtime_probe.py".into()];
    manifest.recipes.push(probe_recipe);
    let probe = crate::project_runtime::ReadinessProbeV1::CustomApprovedProbe {
        recipe_id: "probe-web".into(),
    };
    let command = readiness_command(&manifest, &manifest.candidate_targets[0], &probe).unwrap();
    assert!(command.contains("cd 'tools' && 'python3' 'runtime_probe.py'"));
}

#[test]
fn process_probe_must_match_the_approved_start_executable() {
    let manifest = manifest();
    let probe = crate::project_runtime::ReadinessProbeV1::ProcessAlive {
        executable_id: "node".into(),
    };
    assert!(readiness_command(&manifest, &manifest.candidate_targets[0], &probe).is_err());
}

#[test]
fn go_http_readiness_builds_an_app_owned_stdlib_probe_before_start() {
    let mut manifest = manifest();
    manifest.recipes[0].executable_id = "go".into();
    manifest.recipes[0].prerequisite_tool_ids = vec!["go".into()];
    manifest.recipes[0].argv = vec!["$(touch /tmp/host)".into(), "it's quoted".into()];
    let probe = crate::project_runtime::ReadinessProbeV1::HttpOnApprovedPort {
        port: 8080,
        path: "/health".into(),
        expected_status: 204,
    };

    let setup = readiness_setup_command(&manifest, &manifest.candidate_targets[0], &probe).unwrap();
    assert!(readiness_command(&manifest, &manifest.candidate_targets[0], &probe).is_err());
    let script = start_script(
        &manifest,
        &manifest.candidate_targets[0],
        &manifest.recipes[0],
        &probe,
    )
    .unwrap();

    assert!(setup.contains("printf '%s'"));
    assert!(setup.contains("'go' 'build' '-trimpath'"));
    assert!(setup.contains("func createMemfd"));
    assert!(setup.contains("case \"amd64\":"));
    assert!(setup.contains("case \"arm64\":"));
    assert!(setup.contains("F_SEAL_WRITE"));
    let normalized_setup = setup.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(normalized_setup.contains("prSetDumpable = 4"));
    assert!(setup.contains("syscall.Syscall6(syscall.SYS_PRCTL, prSetDumpable"));
    assert!(setup.contains("PR_GET_DUMPABLE"));
    assert!(script.contains("exec \"$probe_dir/runner\""));
    assert!(script.contains("probe_dir='/run/koolade-runtime-qa/runner-'$$"));
    assert!(script.contains("umask 077"));
    assert!(script.contains("GOTMPDIR=/run/koolade-runtime-qa"));
    assert!(!script.contains("/workspace/.koolade-runtime-qa-"));
    assert!(script.contains("syscall.Exec(\"/proc/self/fd/\""));
    assert!(script.contains("syscall.Wait4(process.Pid"));
    assert!(script.contains("'$(touch /tmp/host)'"));
    assert!(script.contains("'it'\\''s quoted'"));
    let build = script.find("'go' 'build'").unwrap();
    let launch = script.find("exec \"$probe_dir/runner\"").unwrap();
    assert!(
        build < launch,
        "trusted runner is built before project startup"
    );
    assert!(script.contains("approved Go readiness runner build failed"));
    assert!(script.contains("exit 72"));
}
