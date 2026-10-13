use crate::execution::policy::ExecutionPolicyV1;

pub const MAX_SCRIPT_BYTES: usize = 64 * 1024;

pub(super) fn validate_argv(
    argv: &[String],
    has_stdin_script: bool,
    policy: &ExecutionPolicyV1,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        argv.iter().all(|arg| !arg.contains('\0')),
        "argv contains a NUL byte"
    );
    let executable = argv[0].rsplit(['/', '\\']).next().unwrap_or(&argv[0]);
    anyhow::ensure!(
        policy.command_capabilities().contains(executable),
        "executable is outside the policy command capabilities"
    );
    let shell_script = matches!(executable, "sh" | "bash")
        && argv.len() == 2
        && argv[0] == executable
        && argv[1] == "-s";
    anyhow::ensure!(
        if matches!(executable, "sh" | "bash") {
            shell_script && has_stdin_script
        } else {
            !has_stdin_script
                && !argv
                    .iter()
                    .skip(1)
                    .any(|argument| is_shell_command_flag(executable, argument))
        },
        "shell execution requires a bounded script on stdin"
    );
    anyhow::ensure!(
        argv[0] == executable,
        "execution binary must be selected by its image command name"
    );
    for argument in argv {
        anyhow::ensure!(
            !contains_host_path(argument),
            "argv cannot contain controller host paths"
        );
    }
    Ok(())
}

fn is_shell_command_flag(executable: &str, argument: &str) -> bool {
    match executable {
        "sh" | "bash" => {
            argument == "-c"
                || argument == "--command"
                || argument.starts_with("--command=")
                || (argument.starts_with('-')
                    && !argument.starts_with("--")
                    && argument.as_bytes()[1..].contains(&b'c'))
        }
        "pwsh" | "powershell" => {
            let flag = argument.to_ascii_lowercase();
            matches!(flag.as_str(), "-c" | "-command" | "-encodedcommand" | "-e")
                || flag.starts_with("-command:")
                || flag.starts_with("-encodedcommand:")
        }
        _ => false,
    }
}

fn contains_host_path(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.starts_with('/')
        || lower.starts_with("~/")
        || lower.starts_with("~\\")
        || lower.contains("=/")
        || lower.contains("/home/")
        || lower.contains("/users/")
        || lower.contains("\\users\\")
        || (lower.len() >= 3
            && lower.as_bytes()[0].is_ascii_alphabetic()
            && lower.as_bytes()[1] == b':'
            && matches!(lower.as_bytes()[2], b'/' | b'\\'))
}
