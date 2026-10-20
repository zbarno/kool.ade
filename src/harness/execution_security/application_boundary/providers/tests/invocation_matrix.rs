use super::super::CliProvider;
use super::test_boundary;
use crate::harness::{ExecutionMode, ToolAccess};
use std::{fs, time::Duration};

#[cfg(unix)]
#[test]
// Production boundary: the controller-side CLI process environment shared by
// all six provider adapters. The canary proves subprocess lookup does not use
// the task checkout; unit tests of PATH filtering alone cannot prove the real
// clear-environment spawn path applies it. Uses synthetic local executables,
// no Docker/network/authentication dependencies, with low setup cost.
fn fake_provider_invocations_cover_all_modes_and_launch_profiles() {
    use crate::harness::pi_proc::{PollState, StreamEvt, spawn_with_input_provider_env};
    use std::os::unix::fs::PermissionsExt;

    let root =
        std::env::temp_dir().join(format!("koolade-provider-matrix-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let fake_cli = root.join("fake provider cli");
    let workspace = root.join("workspace");
    let workspace_bin = workspace.join("bin");
    fs::create_dir_all(&workspace_bin).unwrap();
    let path_marker = root.join("repository-path-was-executed");
    let fake_command = workspace_bin.join("koolade-path-canary");
    fs::write(
        &fake_command,
        format!("#!/bin/sh\nprintf invoked > '{}'\n", path_marker.display()),
    )
    .unwrap();
    fs::set_permissions(&fake_command, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(
        &fake_cli,
        "#!/bin/sh\ncapture=\"$1\"\nshift\nprintf 'CWD=%s\\nHOME=%s\\n' \"$PWD\" \"$HOME\" > \"$capture\"\nprintf 'ARG=%s\\n' \"$@\" >> \"$capture\"\nenv | sed 's/=.*//' | sort | sed 's/^/ENV_NAME=/' >> \"$capture\"\nprintf 'PATH=%s\\n' \"$PATH\" >> \"$capture\"\nprintf 'MARKER=%s\\n' \"$KOOLADE_TEST_LAUNCH_MARKER\" >> \"$capture\"\nif command -v koolade-path-canary >/dev/null 2>&1; then koolade-path-canary; fi\n",
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
            let host_path = std::env::var("PATH").unwrap_or_default();
            let contaminated_path = std::env::join_paths(
                std::iter::once(workspace_bin.clone())
                    .chain(std::env::split_paths(std::ffi::OsStr::new(&host_path))),
            )
            .unwrap()
            .to_string_lossy()
            .into_owned();
            env.extend([
                ("KOOLADE_TEST_LAUNCH_MARKER".into(), marker.clone()),
                ("PATH".into(), contaminated_path),
            ]);
            let task = spawn_with_input_provider_env(
                &args,
                boundary.working_directory(),
                None,
                &env,
                provider,
                &workspace,
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
                !captured.contains(&workspace_bin.to_string_lossy().to_string()),
                "{provider:?} {mode:?} retained a repository PATH entry"
            );
            assert!(
                !path_marker.exists(),
                "{provider:?} {mode:?} executed a program from repository PATH"
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
