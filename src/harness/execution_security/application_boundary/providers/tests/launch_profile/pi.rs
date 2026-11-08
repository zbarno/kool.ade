use super::{CliProvider, ValidatedProviderLaunch, complete_evidence};
use std::path::Path;

#[test]
fn strict_manager_launch_is_verified_only_for_pi_profile_controls() {
    let provider = CliProvider::Pi;
    let scratch = tempfile::tempdir().unwrap();
    let extension = scratch.path().join("pi-koolade-mcp.ts");
    std::fs::write(&extension, include_bytes!("../../../pi_mcp_extension.ts")).unwrap();
    let requirements = provider.profile_requirements();
    let evidence = complete_evidence(provider, "0.30.1");
    let mut argv = vec!["/bin/true".into()];
    argv.extend(requirements.launch_tokens().iter().map(|token| {
        if token.ends_with('=') {
            format!("{token}read")
        } else {
            (*token).to_owned()
        }
    }));
    let env = vec![
        (
            "PI_CODING_AGENT_DIR".into(),
            scratch
                .path()
                .join("pi-profile")
                .to_string_lossy()
                .into_owned(),
        ),
        ("PI_OFFLINE".into(), "1".into()),
    ];
    argv.extend([
        "--tools".into(),
        "koolade_investigate".into(),
        "--extension".into(),
        extension.to_string_lossy().into_owned(),
    ]);
    ValidatedProviderLaunch::bind_for_workspace(
        evidence,
        crate::harness::ExecutionMode::Manager,
        argv.clone(),
        scratch.path(),
        env.clone(),
        scratch.path(),
    )
    .expect("Pi strict Manager controls are present in the sealed launch");

    let mut missing_builtin_guard = argv.clone();
    missing_builtin_guard.retain(|argument| argument != "--no-builtin-tools");
    assert!(
        ValidatedProviderLaunch::bind_for_workspace(
            complete_evidence(provider, "0.30.1"),
            crate::harness::ExecutionMode::Manager,
            missing_builtin_guard,
            scratch.path(),
            env.clone(),
            scratch.path(),
        )
        .is_err()
    );

    let mut injected_skill = argv.clone();
    injected_skill.extend(["--skill".into(), extension.to_string_lossy().into_owned()]);
    assert!(
        ValidatedProviderLaunch::bind_for_workspace(
            complete_evidence(provider, "0.30.1"),
            crate::harness::ExecutionMode::Manager,
            injected_skill,
            scratch.path(),
            env.clone(),
            scratch.path(),
        )
        .is_err()
    );

    // The Pi CLI accepts option values in `--flag=value` form too. Reject that
    // spelling at the final bind so it cannot hide an extra skill, tool, or
    // extension from the exact-token validation below.
    for injected in [
        format!("--skill={}", extension.display()),
        "--tools=koolade_investigate,koolade_bash".into(),
        "--extension=/tmp/unapproved-extension.ts".into(),
    ] {
        let mut alternate_spelling = argv.clone();
        alternate_spelling.push(injected);
        assert!(
            ValidatedProviderLaunch::bind_for_workspace(
                complete_evidence(provider, "0.30.1"),
                crate::harness::ExecutionMode::Manager,
                alternate_spelling,
                scratch.path(),
                env.clone(),
                scratch.path(),
            )
            .is_err()
        );
    }

    ValidatedProviderLaunch::bind_for_workspace(
        complete_evidence(provider, "0.30.1"),
        crate::harness::ExecutionMode::Manager,
        argv.clone(),
        scratch.path(),
        env.clone(),
        scratch.path(),
    )
    .expect("Pi Manager accepts one isolated app-brokered MCP extension");

    let mut excess_tools = argv.clone();
    let tools_index = excess_tools
        .iter()
        .position(|arg| arg == "--tools")
        .unwrap();
    excess_tools[tools_index + 1] = "koolade_investigate,koolade_bash".into();
    assert!(
        ValidatedProviderLaunch::bind_for_workspace(
            complete_evidence(provider, "0.30.1"),
            crate::harness::ExecutionMode::Manager,
            excess_tools,
            scratch.path(),
            env.clone(),
            scratch.path(),
        )
        .is_err()
    );

    let mut outside_extension = argv.clone();
    let extension_index = outside_extension
        .iter()
        .position(|arg| arg == "--extension")
        .unwrap();
    let other_extension = scratch.path().join("other-extension.ts");
    std::fs::write(
        &other_extension,
        include_bytes!("../../../pi_mcp_extension.ts"),
    )
    .unwrap();
    outside_extension[extension_index + 1] = other_extension.to_string_lossy().into_owned();
    assert!(
        ValidatedProviderLaunch::bind_for_workspace(
            complete_evidence(provider, "0.30.1"),
            crate::harness::ExecutionMode::Manager,
            outside_extension,
            scratch.path(),
            env.clone(),
            scratch.path(),
        )
        .is_err()
    );

    let tool_free = {
        let mut argv = argv;
        for flag in ["--skill", "--extension", "--tools"] {
            if let Some(index) = argv.iter().position(|arg| arg == flag) {
                argv.drain(index..index + 2);
            }
        }
        argv.push("--no-tools".into());
        argv
    };
    assert!(
        ValidatedProviderLaunch::bind_for_workspace(
            complete_evidence(provider, "0.30.1"),
            crate::harness::ExecutionMode::DecisionExplanation,
            tool_free,
            scratch.path(),
            env,
            scratch.path(),
        )
        .is_ok()
    );

    for provider in [
        CliProvider::Antigravity,
        CliProvider::OpenCode,
        CliProvider::Copilot,
    ] {
        let requirements = provider.profile_requirements();
        let mut argv = vec!["/bin/true".into()];
        argv.extend(requirements.launch_tokens().iter().map(|token| {
            if token.ends_with('=') {
                format!("{token}read")
            } else {
                (*token).to_owned()
            }
        }));
        let env = requirements
            .environment_keys()
            .iter()
            .map(|key| ((*key).into(), "/tmp/profile".into()))
            .collect();
        let error = ValidatedProviderLaunch::bind_for_workspace(
            complete_evidence(provider, "99.0.0"),
            crate::harness::ExecutionMode::Manager,
            argv,
            Path::new("/tmp"),
            env,
            Path::new("/tmp"),
        )
        .err()
        .expect("unsupported provider must reject the strict Manager launch");
        assert!(
            error.to_string().contains("not for Man.ager")
                || error.to_string().contains("not verified"),
            "{provider:?}: {error}"
        );
    }
}
