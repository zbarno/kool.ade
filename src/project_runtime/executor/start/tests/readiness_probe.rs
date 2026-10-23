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
