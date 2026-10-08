use std::{fs, path::PathBuf, process::Command};

use crate::harness::pi_sandbox::Sandbox;

use super::support::{TestTree, bwrap_available, create_worktree, run};

#[test]
fn installed_build_tools_keep_their_narrow_shared_runtime_data() {
    if !cfg!(target_os = "linux") || !bwrap_available() {
        return;
    }
    let tree = TestTree::new();
    let (_repository, root) = create_worktree(&tree, "build-runtime");
    fs::write(
        root.join("fixture.cmake"),
        "include(CheckCCompilerFlag)\nif(NOT COMMAND check_c_compiler_flag)\nmessage(FATAL_ERROR \"missing compiler-check module\")\nendif()\n",
    )
    .unwrap();
    let sandbox = Sandbox::new(&root).unwrap();
    let fixtures: &[(&str, &[&str], &str)] = &[
        (
            "perl",
            &["-MCPAN", "-e", "exit 0"],
            "perl -MCPAN -e 'exit 0'",
        ),
        ("cmake", &["--version"], "cmake -P fixture.cmake"),
        ("autoconf", &["--version"], "autoconf --version"),
        ("automake", &["--version"], "automake --version"),
        ("aclocal", &["--version"], "aclocal --version"),
    ];
    for (program, probe, command) in fixtures {
        let Some(executable) = system_executable(program) else {
            continue;
        };
        if !Command::new(executable)
            .args(*probe)
            .output()
            .is_ok_and(|output| output.status.success())
        {
            continue;
        }
        let output = run(&sandbox, "/bin/sh", command);
        assert!(
            output.status.success(),
            "installed {program} lost its runtime data: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

fn system_executable(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let executable = std::env::split_paths(&path)
        .map(|directory| directory.join(program))
        .find(|path| path.is_file())?
        .canonicalize()
        .ok()?;
    super::super::mounts::runtime_visible(&executable).then_some(executable)
}
