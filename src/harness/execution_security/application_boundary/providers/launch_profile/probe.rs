use super::super::CliProvider;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

type ProbeKey = (String, PathBuf, String, u128);
static VERIFIED: OnceLock<Mutex<HashMap<ProbeKey, Vec<String>>>> = OnceLock::new();

pub(super) fn verified_capabilities(
    provider: CliProvider,
    executable: &Path,
    version: &str,
) -> anyhow::Result<Vec<String>> {
    anyhow::ensure!(
        executable.is_absolute(),
        "provider executable identity is not absolute"
    );
    anyhow::ensure!(
        super::parse_version(version).is_some(),
        "{} version evidence is not parseable",
        provider.name()
    );
    let modified = executable
        .metadata()?
        .modified()
        .unwrap_or(SystemTime::UNIX_EPOCH)
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let key = (
        provider.name().to_owned(),
        executable.to_path_buf(),
        version.to_owned(),
        modified,
    );
    let cache = VERIFIED.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(capabilities) = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&key)
        .cloned()
    {
        return Ok(capabilities);
    }

    let requirements = provider.profile_requirements();
    let mut help = String::new();
    for command in requirements.help_commands {
        let mut argv = vec![executable.to_string_lossy().into_owned()];
        if provider == CliProvider::Claude {
            argv.extend([
                "--settings".into(),
                crate::harness::claude_harness::UPDATE_LOCK_SETTINGS.into(),
            ]);
        }
        argv.extend(command.iter().map(|part| (*part).to_owned()));
        let environment = if provider == CliProvider::Claude {
            &[("DISABLE_UPDATES", "1")][..]
        } else {
            &[][..]
        };
        let output = crate::harness::discovery::run_isolated_with_environment(
            &argv,
            Duration::from_secs(10),
            256 * 1024,
            256 * 1024,
            provider.excluded_child_environment(),
            environment,
        )
        .map_err(|error| {
            anyhow::anyhow!(
                "{} clean launch capability probe failed: {error}",
                provider.name()
            )
        })?;
        anyhow::ensure!(
            !output.stdout_truncated && !output.stderr_truncated,
            "{} clean launch capability output exceeded the size limit",
            provider.name()
        );
        anyhow::ensure!(
            output.success,
            "{} does not provide the documented clean launch help interface",
            provider.name()
        );
        help.push_str(&String::from_utf8_lossy(&output.stdout));
        help.push('\n');
        help.push_str(&String::from_utf8_lossy(&output.stderr));
        help.push('\n');
    }

    let capabilities = requirements
        .capabilities
        .iter()
        .copied()
        .filter(|required| capability_present(&help, required))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let missing = requirements
        .missing_capabilities(&capabilities.iter().map(String::as_str).collect::<Vec<_>>());
    anyhow::ensure!(
        missing.is_empty(),
        "{} CLI {version} is unsupported for clean execution; missing options: {}",
        provider.name(),
        missing.join(", ")
    );
    cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(key, capabilities.clone());
    Ok(capabilities)
}

pub(crate) fn capability_present(help: &str, required: &str) -> bool {
    if required.starts_with('-') {
        let required = required.trim_end_matches('=');
        return help.split_whitespace().any(|token| {
            token
                .trim_matches(|character: char| {
                    matches!(character, ',' | ';' | '|' | '[' | ']' | '(' | ')')
                })
                .split(['=', '<', '['])
                .next()
                .is_some_and(|option| option.eq_ignore_ascii_case(required))
        });
    }
    help.split(|character: char| !character.is_ascii_alphanumeric() && character != '-')
        .any(|token| token.eq_ignore_ascii_case(required))
}

#[cfg(test)]
mod tests {
    use super::{capability_present, verified_capabilities};
    use crate::harness::execution_security::CliProvider;
    use std::{fs, path::PathBuf};

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "koolade-launch-capabilities-{}",
                uuid::Uuid::new_v4()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn cli(&self, output: &str) -> PathBuf {
            use std::os::unix::fs::PermissionsExt;
            let binary = self.0.join("fake-cli");
            fs::write(&binary, format!("#!/bin/sh\n{output}\n")).unwrap();
            fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).unwrap();
            binary
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn help_probe_matches_flag_boundaries_and_value_names() {
        assert!(capability_present("  --model <model>  --pure", "--model"));
        assert!(capability_present(
            "--available-tools=TOOL...",
            "--available-tools="
        ));
        assert!(!capability_present("--models list providers", "--model"));
        assert!(capability_present(
            "output format: stream-json",
            "stream-json"
        ));
        assert!(!capability_present("output format: jsonl", "json"));
    }

    #[cfg(unix)]
    #[test]
    fn exact_cli_help_capabilities_gate_each_provider_profile() {
        let fixture = Fixture::new();
        for provider in [
            CliProvider::Pi,
            CliProvider::Codex,
            CliProvider::Claude,
            CliProvider::OpenCode,
            CliProvider::Copilot,
            CliProvider::Antigravity,
        ] {
            let help = provider.profile_requirements().capabilities.join(" ");
            let binary = fixture.cli(&format!(
                "case \"$HOME\" in *koolade-cli-probe*) ;; *) exit 9;; esac\n[ \"$XDG_CONFIG_HOME\" = \"$HOME/config\" ] || exit 10\nprintf '%s\\n' '{help}'"
            ));
            let evidence = verified_capabilities(provider, &binary, &version(provider)).unwrap();
            assert_eq!(
                evidence.iter().map(String::as_str).collect::<Vec<_>>(),
                provider.profile_requirements().capabilities,
                "{provider:?}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn missing_profile_options_report_unsupported_without_launching_a_task() {
        let fixture = Fixture::new();
        let binary = fixture.cli("printf '%s\\n' '--json --sandbox'");
        let error = verified_capabilities(CliProvider::Codex, &binary, "codex 1.2.3").unwrap_err();
        assert!(
            error
                .to_string()
                .contains("unsupported for clean execution")
        );
        assert!(error.to_string().contains("--ignore-user-config"));
    }

    fn version(provider: CliProvider) -> String {
        match provider {
            CliProvider::Claude => "Claude Code 2.1.268".into(),
            _ => format!("{} 99.0.0", provider.name()),
        }
    }
}
