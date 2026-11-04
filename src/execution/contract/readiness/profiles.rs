use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthenticationModeV1 {
    ProviderProfile,
    Environment,
    #[default]
    Unknown,
}

pub fn current_authentication_mode(provider: &str) -> AuthenticationModeV1 {
    authentication_mode_for_sources(provider, |name| std::env::var_os(name).is_some())
}

fn authentication_mode_for_sources(
    provider: &str,
    mut source_is_present: impl FnMut(&str) -> bool,
) -> AuthenticationModeV1 {
    let keys: &[&str] = match provider {
        "pi" => &[
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
            "PI_API_KEY",
        ],
        "codex" => &[
            "OPENAI_API_KEY",
            "CODEX_ACCESS_TOKEN",
            "OPENAI_FEDERATION_RULE_ID",
            "OPENAI_IDENTITY_TOKEN_FILE",
        ],
        "claude" => &[
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_AUTH_TOKEN",
            "CLAUDE_CODE_OAUTH_TOKEN",
        ],
        "antigravity" => &["GOOGLE_API_KEY", "GEMINI_API_KEY"],
        "opencode" => &[
            "OPENAI_API_KEY",
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_AUTH_TOKEN",
            "GOOGLE_API_KEY",
            "GEMINI_API_KEY",
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
        ],
        "copilot" => &["GH_TOKEN", "GITHUB_TOKEN", "COPILOT_GITHUB_TOKEN"],
        _ => return AuthenticationModeV1::Unknown,
    };
    if keys.iter().any(|name| source_is_present(name)) {
        AuthenticationModeV1::Environment
    } else {
        AuthenticationModeV1::ProviderProfile
    }
}

pub fn profile_evidence_id(provider: &str, profile: crate::harness::ExecutionProfile) -> String {
    crate::harness::execution_security::profile_evidence_id(provider, profile)
}

pub fn profile_evidence_id_for_snapshot(
    provider: &str,
    cli_version: Option<&str>,
    authentication_mode: AuthenticationModeV1,
    profile: crate::harness::ExecutionProfile,
) -> String {
    let Some(version) = cli_version
        .map(str::trim)
        .filter(|version| !version.is_empty())
    else {
        return String::new();
    };
    let auth_class = match authentication_mode {
        AuthenticationModeV1::Environment => "environment",
        AuthenticationModeV1::ProviderProfile => "provider-profile",
        AuthenticationModeV1::Unknown => return String::new(),
    };
    let base = profile_evidence_id(provider, profile);
    if base.is_empty() {
        return base;
    }
    format!("{base}@{version}:{auth_class}")
}

pub fn profile_evidence_id_for_current_snapshot(
    provider: &str,
    cli_version: Option<&str>,
    profile: crate::harness::ExecutionProfile,
) -> String {
    profile_evidence_id_for_snapshot(
        provider,
        cli_version,
        current_authentication_mode(provider),
        profile,
    )
}

#[cfg(test)]
#[path = "profiles_tests.rs"]
mod tests;
