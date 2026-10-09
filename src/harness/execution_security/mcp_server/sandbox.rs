use super::ServerConfig;
use std::{
    collections::VecDeque,
    io::Read,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const OUTPUT_LIMIT: usize = 50_000;

pub(super) struct RunOutput {
    pub text: String,
    pub is_error: bool,
}

pub(super) fn run(
    config: &ServerConfig,
    command: &str,
    timeout_seconds: u64,
    offline: bool,
    ecosystem: Option<&str>,
) -> anyhow::Result<RunOutput> {
    let timeout = timeout_seconds.clamp(1, 1200);
    let command = if offline && matches!(ecosystem, Some("npm" | "pnpm" | "yarn")) {
        with_prepared_npm_index(command)
    } else {
        command.to_owned()
    };
    let mut args = config.args.clone();
    if offline {
        for (name, value) in [
            ("npm_config_offline", "true"),
            ("npm_config_registry", "https://registry.npmjs.org/"),
            ("npm_config_userconfig", "/dev/null"),
            ("npm_config_globalconfig", "/tmp/koolade-home/.npm-globalrc"),
            ("npm_config_audit", "false"),
            ("CARGO_NET_OFFLINE", "true"),
        ] {
            args.extend(["--setenv".into(), name.into(), value.into()]);
        }
    }
    args.extend([
        "--".into(),
        "/bin/bash".into(),
        "-c".into(),
        "ulimit -u 1024; ulimit -f 2097152; ulimit -c 0; exec \"$1\" -c \"$2\"".into(),
        "koolade-sandbox".into(),
        "/bin/bash".into(),
        command,
    ]);
    let mut child = Command::new(&config.bwrap)
        .args(args)
        .current_dir("/")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");
    let stdout_reader = thread::spawn(move || read_tail(stdout));
    let stderr_reader = thread::spawn(move || read_tail(stderr));
    let deadline = Instant::now() + Duration::from_secs(timeout);
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if Instant::now() >= deadline {
            timed_out = true;
            let _ = child.kill();
            break child.wait().ok();
        }
        thread::sleep(Duration::from_millis(100));
    };
    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();
    let code = status.and_then(|status| status.code());
    let mut parts = Vec::new();
    if !stdout.trim().is_empty() {
        parts.push(stdout.trim().to_owned());
    }
    if !stderr.trim().is_empty() {
        parts.push(stderr.trim().to_owned());
    }
    if timed_out {
        parts.push(format!("Command timed out after {timeout} seconds."));
    }
    parts.push(format!(
        "Exit code: {}",
        code.map_or_else(|| "unknown".into(), |code| code.to_string())
    ));
    Ok(RunOutput {
        text: parts.join("\n"),
        is_error: timed_out || code != Some(0),
    })
}

fn read_tail(mut reader: impl Read) -> String {
    let mut tail = VecDeque::with_capacity(OUTPUT_LIMIT);
    let mut chunk = [0; 8192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(count) => {
                for byte in &chunk[..count] {
                    if tail.len() == OUTPUT_LIMIT {
                        tail.pop_front();
                    }
                    tail.push_back(*byte);
                }
            }
        }
    }
    String::from_utf8_lossy(&tail.into_iter().collect::<Vec<_>>()).into_owned()
}

fn with_prepared_npm_index(command: &str) -> String {
    format!(
        r#"mkdir -p /tmp/koolade-home/.npm-prepared/_cacache/index-v5 /tmp/koolade-home/.npm-prepared/_cacache/tmp && generation=$(cat /tmp/koolade-home/.npm-prepared/_cacache/index-source-v5/current) && case "$generation" in ''|*[!0-9a-f-]*) echo 'Prepared npm snapshot pointer is invalid.' >&2; exit 1;; esac && test "${{#generation}}" = 36 && source=/tmp/koolade-home/.npm-prepared/_cacache/index-source-v5/$generation/index-v5 && if find "$source" -type l -print -quit | grep -q .; then echo 'Prepared npm index contains a symlink.' >&2; exit 1; fi && cp -R "$source"/. /tmp/koolade-home/.npm-prepared/_cacache/index-v5/ && set -f && {command}"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn npm_retry_validates_prepared_snapshot_before_running_worker_command() {
        let command = with_prepared_npm_index("npm ci");
        assert!(command.contains("test \"${#generation}\" = 36"));
        assert!(command.contains("find \"$source\" -type l"));
        assert!(command.ends_with("&& set -f && npm ci"));
    }
}
