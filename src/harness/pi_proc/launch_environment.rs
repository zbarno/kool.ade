use std::path::Path;

use crate::harness::execution_security::CliProvider;

const RUNTIME_ENV: &[&str] = &[
    "TMPDIR",
    "TMP",
    "TEMP",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "LC_MESSAGES",
    "LC_NUMERIC",
    "LC_TIME",
    "TERM",
    "COLORTERM",
    "NO_COLOR",
    "SYSTEMROOT",
    "WINDIR",
    "COMSPEC",
    "PATHEXT",
];

const API_AUTH_ENV: &[&str] = &[
    "OPENAI_API_KEY",
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "GEMINI_API_KEY",
    "GOOGLE_API_KEY",
    "OPENROUTER_API_KEY",
    "GROQ_API_KEY",
    "DEEPSEEK_API_KEY",
    "MISTRAL_API_KEY",
    "XAI_API_KEY",
    "TOGETHER_API_KEY",
    "FIREWORKS_API_KEY",
    "CEREBRAS_API_KEY",
    "PERPLEXITY_API_KEY",
    "COHERE_API_KEY",
];

const COPILOT_AUTH_ENV: &[&str] = &["GH_TOKEN", "GITHUB_TOKEN", "COPILOT_GITHUB_TOKEN"];

pub(crate) fn provider_environment(
    provider: CliProvider,
    cwd: &Path,
    workspace_root: &Path,
    explicit: &[(String, String)],
) -> anyhow::Result<Vec<(String, String)>> {
    let mut result = filter_environment(provider, std::env::vars());
    if !result.iter().any(|(name, _)| name == "HOME") {
        result.push(("HOME".into(), cwd.to_string_lossy().into_owned()));
    }
    let requested_path = explicit
        .iter()
        .rev()
        .find(|(name, _)| name == "PATH")
        .map(|(_, value)| value.as_str())
        .map(str::to_owned)
        .or_else(|| std::env::var("PATH").ok())
        .unwrap_or_default();
    for (name, value) in explicit {
        if name == "PATH" {
            continue;
        }
        result.retain(|(existing, _)| existing != name);
        result.push((name.clone(), value.clone()));
    }
    result.retain(|(name, _)| name != "PATH");
    result.push((
        "PATH".into(),
        safe_provider_path(&requested_path, cwd, workspace_root)?,
    ));
    Ok(result)
}

fn safe_provider_path(value: &str, cwd: &Path, workspace_root: &Path) -> anyhow::Result<String> {
    let blocked_roots = [cwd.canonicalize()?, workspace_root.canonicalize()?];
    let mut accepted = Vec::new();
    for entry in std::env::split_paths(std::ffi::OsStr::new(value)) {
        // Relative and empty PATH entries resolve against the child CWD, so
        // they can make repository files executable during provider startup.
        if !entry.is_absolute() {
            continue;
        }
        let Ok(entry) = entry.canonicalize() else {
            continue;
        };
        if !entry.is_dir()
            || blocked_roots.iter().any(|root| entry.starts_with(root))
            || !trusted_runtime_directory(&entry)
            || accepted.contains(&entry)
        {
            continue;
        }
        accepted.push(entry);
    }
    Ok(std::env::join_paths(accepted)
        .map_err(|error| anyhow::anyhow!("clean provider PATH is invalid: {error}"))?
        .to_string_lossy()
        .into_owned())
}

#[cfg(unix)]
fn trusted_runtime_directory(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;

    path.ancestors().all(|ancestor| {
        ancestor
            .metadata()
            .is_ok_and(|metadata| metadata.uid() == 0 && metadata.mode() & 0o022 == 0)
    })
}

#[cfg(windows)]
fn trusted_runtime_directory(path: &Path) -> bool {
    let Some(system_root) = std::env::var_os("SystemRoot") else {
        return false;
    };
    let Ok(system_root) = Path::new(&system_root).canonicalize() else {
        return false;
    };
    path == system_root
        || system_root
            .join("System32")
            .canonicalize()
            .is_ok_and(|system32| path == system32)
}

#[cfg(not(any(unix, windows)))]
fn trusted_runtime_directory(_: &Path) -> bool {
    false
}

fn filter_environment(
    provider: CliProvider,
    source: impl IntoIterator<Item = (String, String)>,
) -> Vec<(String, String)> {
    source
        .into_iter()
        .filter(|(name, _)| {
            RUNTIME_ENV.contains(&name.as_str())
                || match provider {
                    CliProvider::Copilot => COPILOT_AUTH_ENV.contains(&name.as_str()),
                    CliProvider::Codex => {
                        matches!(name.as_str(), "OPENAI_API_KEY" | "CODEX_ACCESS_TOKEN")
                    }
                    CliProvider::Claude => {
                        matches!(name.as_str(), "ANTHROPIC_API_KEY" | "ANTHROPIC_AUTH_TOKEN")
                    }
                    CliProvider::Antigravity => name == "GEMINI_API_KEY",
                    CliProvider::OpenCode | CliProvider::Pi => {
                        API_AUTH_ENV.contains(&name.as_str())
                    }
                }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selected(provider: CliProvider, input: &[(&str, &str)]) -> Vec<(String, String)> {
        let input: Vec<_> = input
            .iter()
            .map(|(key, value)| ((*key).into(), (*value).into()))
            .collect();
        let mut result = filter_environment(provider, input);
        if !result.iter().any(|(name, _)| name == "HOME") {
            result.push(("HOME".into(), "/private/run".into()));
        }
        result
    }

    #[test]
    fn filters_host_injection_but_keeps_runtime_and_provider_auth() {
        let source = [
            ("PATH", "/usr/bin"),
            ("TMPDIR", "/tmp"),
            ("TEMP", "/tmp"),
            ("LANG", "C.UTF-8"),
            ("LC_ALL", "C.UTF-8"),
            ("NODE_OPTIONS", "--require=/tmp/inject.js"),
            ("GIT_CONFIG", "/tmp/gitconfig"),
            ("GIT_CONFIG_GLOBAL", "/tmp/gitconfig"),
            ("CODEX_HOME", "/home/host/.codex"),
            ("CLAUDE_CONFIG_DIR", "/home/host/.claude"),
            ("ANTHROPIC_CONFIG_DIR", "/home/host/.config/claude"),
            ("COPILOT_HOME", "/home/host/.copilot"),
            ("OPENCODE_CONFIG", "/tmp/opencode.json"),
            ("OPENCODE_CONFIG_CONTENT", "{}"),
            ("GOOGLE_APPLICATION_CREDENTIALS", "/tmp/credentials.json"),
            ("AWS_SHARED_CREDENTIALS_FILE", "/tmp/aws"),
            ("AZURE_CONFIG_DIR", "/home/host/.azure"),
            ("NPM_CONFIG_USERCONFIG", "/home/host/.npmrc"),
            ("CARGO_HOME", "/home/host/.cargo"),
            ("HOME", "/home/host"),
            ("OPENAI_API_KEY", "synthetic-key"),
            ("CODEX_ACCESS_TOKEN", "synthetic-codex-token"),
        ];
        let env = selected(CliProvider::Codex, &source);
        for required in [
            "TMPDIR",
            "TEMP",
            "LANG",
            "LC_ALL",
            "OPENAI_API_KEY",
            "CODEX_ACCESS_TOKEN",
        ] {
            assert!(env.iter().any(|(name, _)| name == required), "{required}");
        }
        for denied in [
            "PATH",
            "NODE_OPTIONS",
            "GIT_CONFIG",
            "GIT_CONFIG_GLOBAL",
            "CODEX_HOME",
            "CLAUDE_CONFIG_DIR",
            "ANTHROPIC_CONFIG_DIR",
            "COPILOT_HOME",
            "OPENCODE_CONFIG",
            "OPENCODE_CONFIG_CONTENT",
            "GOOGLE_APPLICATION_CREDENTIALS",
            "AWS_SHARED_CREDENTIALS_FILE",
            "AZURE_CONFIG_DIR",
            "NPM_CONFIG_USERCONFIG",
            "CARGO_HOME",
        ] {
            assert!(!env.iter().any(|(name, _)| name == denied), "{denied}");
        }
        assert_eq!(
            env.iter().find(|(name, _)| name == "HOME").unwrap().1,
            "/private/run"
        );
    }

    #[test]
    fn codex_access_token_is_not_forwarded_to_other_providers() {
        let source = [("CODEX_ACCESS_TOKEN", "synthetic-codex-token")]
            .map(|(name, value)| (name.to_owned(), value.to_owned()));
        for provider in [
            CliProvider::Pi,
            CliProvider::Claude,
            CliProvider::Antigravity,
            CliProvider::OpenCode,
            CliProvider::Copilot,
        ] {
            assert!(
                filter_environment(provider, source.clone()).is_empty(),
                "{provider:?} received Codex-only credentials"
            );
        }
    }

    #[test]
    fn app_generated_runtime_values_override_the_baseline() {
        let cwd = std::env::temp_dir();
        let env = provider_environment(
            CliProvider::Pi,
            &cwd,
            &cwd,
            &[("PI_OFFLINE".into(), "1".into())],
        )
        .unwrap();
        assert!(
            env.iter()
                .any(|(name, value)| name == "PI_OFFLINE" && value == "1")
        );
        assert!(env.iter().any(|(name, _)| name == "PATH"));
    }

    #[test]
    fn antigravity_does_not_receive_an_unsupported_google_api_key_alias() {
        let source = [
            ("GOOGLE_API_KEY", "unused-alias"),
            ("GEMINI_API_KEY", "explicit-gemini-key"),
        ]
        .map(|(name, value)| (name.to_owned(), value.to_owned()));
        let env = filter_environment(CliProvider::Antigravity, source);
        assert!(
            env.iter().any(|(name, value)| {
                name == "GEMINI_API_KEY" && value == "explicit-gemini-key"
            })
        );
        assert!(!env.iter().any(|(name, _)| name == "GOOGLE_API_KEY"));
    }

    #[cfg(unix)]
    #[test]
    fn provider_path_removes_repository_relative_and_symlink_entries() {
        use std::os::unix::fs::symlink;

        let root =
            std::env::temp_dir().join(format!("koolade-provider-path-{}", uuid::Uuid::new_v4()));
        let workspace = root.join("workspace");
        let repo_bin = workspace.join("bin");
        let user_bin = root.join("user-bin");
        let private_cwd = root.join("private-cwd");
        for directory in [&repo_bin, &user_bin, &private_cwd] {
            std::fs::create_dir_all(directory).unwrap();
        }
        let repo_alias = root.join("repo-bin-alias");
        symlink(&repo_bin, &repo_alias).unwrap();
        let input = std::env::join_paths([
            repo_bin.as_path(),
            repo_alias.as_path(),
            user_bin.as_path(),
            Path::new("/usr/bin"),
            std::path::Path::new("relative-bin"),
            std::path::Path::new(""),
        ])
        .unwrap();

        let filtered =
            safe_provider_path(&input.to_string_lossy(), &private_cwd, &workspace).unwrap();
        let entries = std::env::split_paths(std::ffi::OsStr::new(&filtered)).collect::<Vec<_>>();
        assert!(!entries.contains(&repo_bin.canonicalize().unwrap()));
        assert!(!entries.contains(&repo_alias.canonicalize().unwrap()));
        assert!(!entries.contains(&user_bin.canonicalize().unwrap()));
        assert!(entries.iter().all(|entry| trusted_runtime_directory(entry)));
        let system_bin = Path::new("/usr/bin").canonicalize();
        if let Ok(system_bin) = system_bin
            && trusted_runtime_directory(&system_bin)
        {
            assert!(entries.contains(&system_bin));
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn windows_provider_path_keeps_only_system_root_and_system32() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let system_root = std::env::var_os("SystemRoot")
            .expect("Windows should provide SystemRoot")
            .into_string()
            .expect("SystemRoot should be Unicode");
        let system_root = Path::new(&system_root).canonicalize().unwrap();
        let system32 = system_root.join("System32").canonicalize().unwrap();
        let test_root = std::env::temp_dir().join(format!(
            "koolade-provider-path-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let workspace = test_root.join("workspace");
        let repo_bin = workspace.join("bin");
        let user_bin = test_root.join("user-bin");
        let private_cwd = test_root.join("private-cwd");
        for directory in [&repo_bin, &user_bin, &private_cwd] {
            std::fs::create_dir_all(directory).unwrap();
        }
        let input = std::env::join_paths([
            system_root.as_path(),
            system32.as_path(),
            user_bin.as_path(),
            std::path::Path::new("relative-bin"),
        ])
        .unwrap();

        let filtered =
            safe_provider_path(&input.to_string_lossy(), &private_cwd, &workspace).unwrap();
        let entries = std::env::split_paths(std::ffi::OsStr::new(&filtered)).collect::<Vec<_>>();
        assert_eq!(entries, [system_root, system32]);
        std::fs::remove_dir_all(test_root).unwrap();
    }
}
