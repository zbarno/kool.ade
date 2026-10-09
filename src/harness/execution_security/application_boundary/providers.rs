mod antigravity;
mod claude;
mod codex;
mod copilot;
mod managed_hooks;
mod opencode;
mod shared;

#[cfg(test)]
mod tests;

use super::ApplicationBoundary;

#[derive(Clone, Copy, Debug)]
pub(crate) enum CliProvider {
    Codex,
    Claude,
    Antigravity,
    OpenCode,
    Copilot,
}

impl CliProvider {
    pub(crate) fn excluded_child_environment(self) -> &'static [&'static str] {
        const NODE_STARTUP_ENV: &[&str] = &["NODE_OPTIONS"];
        const CLAUDE_STARTUP_ENV: &[&str] = &["NODE_OPTIONS", "CLAUDE_CODE_SHELL_PREFIX"];
        match self {
            Self::Claude => CLAUDE_STARTUP_ENV,
            Self::Codex | Self::Antigravity | Self::OpenCode | Self::Copilot => NODE_STARTUP_ENV,
        }
    }
}

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
    }
    Ok(())
}
