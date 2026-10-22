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
    pub(crate) fn from_id(id: &str) -> Option<Self> {
        match id {
            "codex" => Some(Self::Codex),
            "claude" => Some(Self::Claude),
            "antigravity" => Some(Self::Antigravity),
            "opencode" => Some(Self::OpenCode),
            "copilot" => Some(Self::Copilot),
            "pi" => Some(Self::Pi),
            _ => None,
        }
    }

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

    pub(super) fn telemetry_id(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Antigravity => "antigravity",
            Self::OpenCode => "opencode",
            Self::Copilot => "copilot",
            Self::Pi => "pi",
        }
    }

    /// Stable prerequisites for each clean launch profile. A validated launch
    /// plan binds the successful probe to the configured argv and environment.
    pub(crate) fn profile_requirements(self) -> ProfileRequirements {
        match self {
            Self::Claude => ProfileRequirements {
                minimum_version: Some((2, 1, 268)),
                help_commands: &[&["--help"]],
                capabilities: &[
                    "-p",
                    "--output-format",
                    "--input-format",
                    "--verbose",
                    "--no-session-persistence",
                    "--restricted",
                    "--permission-prompts",
                    "--tools",
                    "--permission-mode",
                    "--effort",
                    "--strict-mcp-config",
                    "--mcp-config",
                    "--allowedTools",
                ],
                launch_tokens: &[
                    "--tools",
                    "--restricted",
                    "--strict-mcp-config",
                    "--permission-mode",
                    "dontAsk",
                    "--effort",
                ],
                environment_keys: &[],
            },
            Self::Codex => ProfileRequirements {
                minimum_version: None,
                help_commands: &[&["exec", "--help"]],
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
                launch_tokens: &[
                    "--sandbox",
                    "--ignore-user-config",
                    "--skip-git-repo-check",
                    "--config",
                ],
                environment_keys: &[],
            },
            Self::Pi => ProfileRequirements {
                minimum_version: None,
                help_commands: &[&["--help"]],
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
                    "--thinking",
                ],
                environment_keys: &["PI_CODING_AGENT_DIR", "PI_OFFLINE"],
            },
            Self::Antigravity => ProfileRequirements {
                minimum_version: None,
                help_commands: &[&["--help"]],
                capabilities: &[
                    "--input-format",
                    "--output-format",
                    "--model",
                    "--effort",
                    "--sandbox",
                ],
                launch_tokens: &[
                    "--input-format",
                    "stream-json",
                    "--output-format",
                    "stream-json",
                    "--sandbox",
                    "--effort",
                ],
                environment_keys: &["HOME", "XDG_CONFIG_HOME"],
            },
            Self::OpenCode => ProfileRequirements {
                minimum_version: None,
                help_commands: &[&["--help"], &["run", "--help"]],
                capabilities: &[
                    "--pure",
                    "run",
                    "--format",
                    "--auto",
                    "--dir",
                    "--agent",
                    "--model",
                    "--variant",
                ],
                launch_tokens: &[
                    "--pure",
                    "run",
                    "--format",
                    "json",
                    "--auto",
                    "--dir",
                    "--variant",
                ],
                environment_keys: &["HOME", "OPENCODE_CONFIG", "OPENCODE_CONFIG_CONTENT"],
            },
            Self::Copilot => ProfileRequirements {
                minimum_version: None,
                help_commands: &[&["--help"]],
                capabilities: &[
                    "--silent",
                    "--no-ask-user",
                    "--no-auto-update",
                    "--available-tools",
                    "--effort",
                    "--additional-mcp-config",
                    "--disable-builtin-mcps",
                    "--no-custom-instructions",
                    "--no-remote",
                    "--no-remote-export",
                    "--allow-tool",
                ],
                launch_tokens: &[
                    "--silent",
                    "--no-ask-user",
                    "--no-auto-update",
                    "--available-tools=",
                    "--effort",
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
