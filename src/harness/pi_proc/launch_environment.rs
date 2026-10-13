use std::path::Path;

use crate::harness::execution_security::CliProvider;

const RUNTIME_ENV: &[&str] = &[
    "PATH",
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
    explicit: &[(String, String)],
) -> Vec<(String, String)> {
    let mut result = filter_environment(provider, std::env::vars());
    if !result.iter().any(|(name, _)| name == "HOME") {
        result.push(("HOME".into(), cwd.to_string_lossy().into_owned()));
    }
    for (name, value) in explicit {
        result.retain(|(existing, _)| existing != name);
        result.push((name.clone(), value.clone()));
    }
    result
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
                    CliProvider::Codex => name == "OPENAI_API_KEY",
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
        ];
        let env = selected(CliProvider::Codex, &source);
        for required in ["PATH", "TMPDIR", "TEMP", "LANG", "LC_ALL", "OPENAI_API_KEY"] {
            assert!(env.iter().any(|(name, _)| name == required), "{required}");
        }
        for denied in [
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
    fn app_generated_runtime_values_override_the_baseline() {
        let env = provider_environment(
            CliProvider::Pi,
            Path::new("/private/run"),
            &[("PI_OFFLINE".into(), "1".into())],
        );
        assert!(
            env.iter()
                .any(|(name, value)| name == "PI_OFFLINE" && value == "1")
        );
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
}
