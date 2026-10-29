use super::super::validate_start_executable_artifact;
use super::*;
use crate::project_runtime::ReadinessProbeV1;

#[test]
fn start_recipe_can_launch_an_approved_target_relative_binary_without_arguments() {
    let mut start = recipe();
    start.executable_id = "dist/my-app".into();
    start.argv.clear();
    assert!(start.validate().is_ok());

    let mut manifest = manifest();
    manifest.recipes[0] = start.clone();
    let probe = ReadinessProbeV1::ProcessAlive {
        executable_id: "dist/my-app".into(),
    };
    let script = start_script(&manifest, &target(), &start, &probe).unwrap();
    assert!(script.contains("exec 'dist/my-app'"));
    assert!(!script.contains("dist/my-app' ''"));
}

#[test]
fn start_executable_path_must_be_in_the_approved_build_artifact_set() {
    let mut start = recipe();
    start.working_directory_relative = ".".into();
    start.executable_id = "dist/my-app".into();
    start.argv.clear();
    assert!(validate_start_executable_artifact(&start, &["dist/my-app".into()]).is_ok());
    assert!(validate_start_executable_artifact(&start, &["dist/other".into()]).is_err());

    start.working_directory_relative = "apps/web".into();
    assert!(validate_start_executable_artifact(&start, &["apps/web/dist/my-app".into()]).is_ok());
    assert!(validate_start_executable_artifact(&start, &["dist/my-app".into()]).is_err());
}

#[test]
fn start_can_prove_a_discovered_target_is_ready() {
    assert!(require_startable_state(&TargetCapabilityState::ReadyButUntested).is_ok());
    for state in [
        TargetCapabilityState::MissingToolchain,
        TargetCapabilityState::MissingDependencies,
        TargetCapabilityState::DriverUnavailable,
    ] {
        assert!(require_startable_state(&state).is_ok(), "{state:?}");
    }
    for state in [
        TargetCapabilityState::UnsupportedPlatform,
        TargetCapabilityState::UnauthorizedRecipe,
        TargetCapabilityState::BuildFailed,
        TargetCapabilityState::StartFailed,
    ] {
        assert!(require_startable_state(&state).is_err(), "{state:?}");
    }
}
