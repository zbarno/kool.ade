mod antigravity;
mod claude;
mod codex;
mod copilot;
mod launch_profile;
mod managed_hooks;
mod opencode;
mod pi;
mod shared;

#[cfg(test)]
mod tests;

use super::ApplicationBoundary;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CliProvider {
    Codex,
    Claude,
    Antigravity,
    OpenCode,
    Copilot,
    Pi,
}

impl CliProvider {
    fn name(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::Claude => "Claude Code",
            Self::Antigravity => "Antigravity",
            Self::OpenCode => "OpenCode",
            Self::Copilot => "GitHub Copilot CLI",
            Self::Pi => "Pi",
        }
    }

    /// Stable prerequisites for each clean launch profile. A validated launch
    /// plan binds the successful probe to the configured argv and environment.
    pub(crate) fn profile_requirements(self) -> ProfileRequirements {
        match self {
            Self::Claude => ProfileRequirements {
                minimum_version: Some((2, 1, 268)),
                capabilities: &[],
                launch_tokens: &[
                    "--tools",
                    "--restricted",
                    "--strict-mcp-config",
                    "--permission-mode",
                    "dontAsk",
                ],
                environment_keys: &[],
            },
            Self::Codex => ProfileRequirements {
                minimum_version: None,
                capabilities: &[
                    "--json",
                    "--cd",
                    "--sandbox",
                    "--ephemeral",
                    "--model",
                    "--config",
                    "--disable",
                    "--ignore-user-config",
                    "--skip-git-repo-check",
                ],
                launch_tokens: &["--sandbox", "--ignore-user-config", "--skip-git-repo-check"],
                environment_keys: &[],
            },
            Self::Pi => ProfileRequirements {
                minimum_version: None,
                capabilities: &[
                    "--print",
                    "--mode",
                    "--no-session",
                    "--no-approve",
                    "--append-system-prompt",
                    "--thinking",
                    "--provider",
                    "--model",
                    "--no-extensions",
                    "--no-skills",
                    "--no-prompt-templates",
                    "--no-themes",
                    "--no-context-files",
                    "--no-tools",
                    "--no-builtin-tools",
                    "--tools",
                    "--extension",
                    "json",
                    "xhigh",
                ],
                launch_tokens: &[
                    "--no-session",
                    "--no-approve",
                    "--no-context-files",
                    "--no-extensions",
                    "--no-skills",
                    "--no-prompt-templates",
                    "--no-themes",
                ],
                environment_keys: &["PI_CODING_AGENT_DIR", "PI_OFFLINE"],
            },
            Self::Antigravity => ProfileRequirements {
                minimum_version: None,
                capabilities: &[],
                launch_tokens: &[
                    "--input-format",
                    "stream-json",
                    "--output-format",
                    "stream-json",
                    "--sandbox",
                ],
                environment_keys: &["HOME", "XDG_CONFIG_HOME"],
            },
            Self::OpenCode => ProfileRequirements {
                minimum_version: None,
                capabilities: &[],
                launch_tokens: &["--pure", "run", "--format", "json", "--auto", "--dir"],
                environment_keys: &["HOME", "OPENCODE_CONFIG", "OPENCODE_CONFIG_CONTENT"],
            },
            Self::Copilot => ProfileRequirements {
                minimum_version: None,
                capabilities: &[],
                launch_tokens: &[
                    "--silent",
                    "--no-ask-user",
                    "--no-auto-update",
                    "--available-tools=",
                ],
                environment_keys: &["COPILOT_HOME", "COPILOT_CACHE_HOME"],
            },
        }
    }

    pub(crate) fn excluded_child_environment(self) -> &'static [&'static str] {
        let _ = self;
        super::super::UNSAFE_PROVIDER_LAUNCH_ENV
    }
}

pub(crate) use launch_profile::{
    ProfileProbeEvidence, ProfileRequirements, ValidatedProviderLaunch,
};

pub(super) fn configure(
    boundary: &ApplicationBoundary,
    provider: CliProvider,
    argv: &mut Vec<String>,
    env: &mut Vec<(String, String)>,
) -> anyhow::Result<()> {
    match provider {
        CliProvider::Codex => codex::configure(boundary, argv)?,
        CliProvider::Claude => claude::configure(boundary, argv)?,
        CliProvider::Antigravity => antigravity::configure(boundary, argv, env)?,
        CliProvider::OpenCode => opencode::configure(boundary, argv, env)?,
        CliProvider::Copilot => copilot::configure(boundary, argv, env)?,
        CliProvider::Pi => pi::configure(boundary, argv, env)?,
    }
    Ok(())
}
