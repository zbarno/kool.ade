use super::super::CliProvider;
use super::test_boundary;
use crate::harness::{ExecutionMode, ToolAccess};
use std::{fs, time::Duration};

#[cfg(unix)]
#[test]
fn fake_provider_invocations_cover_all_modes_and_launch_profiles() {
    use crate::harness::pi_proc::{PollState, StreamEvt, spawn_with_input_provider_env};
    use std::os::unix::fs::PermissionsExt;

    let root =
        std::env::temp_dir().join(format!("koolade-provider-matrix-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let fake_cli = root.join("fake provider cli");
    fs::write(
        &fake_cli,
        "#!/bin/sh\ncapture=\"$1\"\nshift\nprintf 'CWD=%s\\nHOME=%s\\n' \"$PWD\" \"$HOME\" > \"$capture\"\nprintf 'ARG=%s\\n' \"$@\" >> \"$capture\"\nenv | sed 's/=.*//' | sort | sed 's/^/ENV_NAME=/' >> \"$capture\"\nprintf 'MARKER=%s\\n' \"$KOOLADE_TEST_LAUNCH_MARKER\" >> \"$capture\"\n",
    )
    .unwrap();
    fs::set_permissions(&fake_cli, fs::Permissions::from_mode(0o700)).unwrap();

    let providers = [
        CliProvider::Pi,
        CliProvider::Codex,
        CliProvider::Claude,
        CliProvider::OpenCode,
        CliProvider::Copilot,
        CliProvider::Antigravity,
    ];
    for mode in ExecutionMode::ALL {
        let access = mode.tool_access();
        for provider in providers {
            let boundary = test_boundary(access);
            let mut args = vec![fake_cli.to_string_lossy().into_owned()];
            let capture = root.join(format!("{:?}-{:?}.capture", provider, mode));
            args.push(capture.to_string_lossy().into_owned());
            let mut env = Vec::new();
            boundary.configure(provider, &mut args, &mut env).unwrap();
            let marker = format!("synthetic-{:?}-{:?}", provider, mode);
            env.push(("KOOLADE_TEST_LAUNCH_MARKER".into(), marker.clone()));
            let task = spawn_with_input_provider_env(
                &args,
                boundary.working_directory(),
                None,
                &env,
                provider,
            )
            .unwrap();
            loop {
                match task.poll_next(Duration::from_millis(100)) {
                    Ok(StreamEvt::Exited(true)) => {
                        assert!(capture.exists(), "{provider:?} {mode:?} did not run");
                        break;
                    }
                    Ok(StreamEvt::Exited(false)) => {
                        panic!("{provider:?} failed in {mode:?}")
                    }
                    Ok(StreamEvt::Stdout(_) | StreamEvt::Stderr(_)) | Err(PollState::Pending) => {}
                    Err(PollState::Closed) => panic!("{provider:?} {mode:?} closed early"),
                }
            }
            let captured = fs::read_to_string(&capture).unwrap();
            assert!(
                captured.contains(&format!("CWD={}", boundary.working_directory().display())),
                "{provider:?} {mode:?} did not use the private launch cwd"
            );
            for (name, _) in &env {
                assert!(
                    captured.contains(&format!("ENV_NAME={name}")),
                    "{provider:?} {mode:?} dropped configured environment variable {name}"
                );
            }
            assert!(
                captured.contains(&format!("MARKER={marker}")),
                "{provider:?} {mode:?} dropped the synthetic launch marker"
            );
            assert!(
                !captured.contains("ENV_NAME=NODE_OPTIONS")
                    && !captured.contains("ENV_NAME=GIT_CONFIG_GLOBAL"),
                "{provider:?} {mode:?} inherited a forbidden launch variable"
            );
            assert_eq!(
                boundary.mcp_tool_names().is_empty(),
                access == ToolAccess::None,
                "{provider:?} {mode:?} MCP authority did not match its mode"
            );
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[cfg(not(unix))]
#[test]
fn fake_provider_invocation_matrix_requires_unix_process_groups() {}
