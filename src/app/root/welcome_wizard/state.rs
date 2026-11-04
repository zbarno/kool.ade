/// The first-party journey starters offered on the welcome landing.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::app::root) enum WelcomeEntry {
    /// Clone a code repository into Kool.ad/e-managed locations.
    CloneRepository,
}

impl WelcomeEntry {
    pub(super) fn title(self) -> &'static str {
        match self {
            Self::CloneRepository => "Clone a code repository",
        }
    }
}

/// Draft state for ONE active welcome wizard. Closing a draft discards it;
/// nothing touches the disk until the confirm step commits.
pub(in crate::app::root) struct WelcomeWizard {
    pub(super) entry: WelcomeEntry,
    /// 0 = gather inputs, 1 = confirm the planned side effects.
    pub(super) step: u8,
    /// CloneRepository: the repository URL to download (cloned into
    /// `~/kool-ade-workspaces/<repo>` by the shared clone engine).
    pub(super) url: String,
    pub(super) error: Option<String>,
}

impl WelcomeWizard {
    pub(super) fn new(entry: WelcomeEntry) -> Self {
        Self {
            entry,
            step: 0,
            url: String::new(),
            error: None,
        }
    }

    /// Absolute destination the clone engine will use for CloneRepository.
    pub(super) fn clone_destination(&self) -> Option<std::path::PathBuf> {
        let home = std::env::var_os("HOME")?;
        let name = self.code_folder_name();
        if name.is_empty() {
            return None;
        }
        Some(crate::app::welcome::github::clone_destination(
            &name,
            &std::path::PathBuf::from(home),
        ))
    }

    /// Code folder name where the code repository will live after commit.
    pub(super) fn code_folder_name(&self) -> String {
        crate::app::welcome::parse_github_url(self.url.trim())
            .map(|target| target.repo)
            .unwrap_or_default()
    }

    /// Human-readable list of the effects confirmed by the commit step.
    pub(super) fn disk_plan_lines(&self) -> Vec<String> {
        let name = self.code_folder_name();
        let code_repository = crate::app::welcome::parse_github_url(self.url.trim())
            .map(|target| format!("{}/{}", target.owner, target.repo))
            .unwrap_or_else(|_| name.clone());
        let shown = match self.clone_destination() {
            Some(dest) => dest.display().to_string(),
            None => format!("$HOME/kool-ade-workspaces/{name}"),
        };
        vec![format!(
            "Clones GitHub repository “{code_repository}” into “{shown}”."
        )]
    }
}

#[cfg(test)]
mod tests;
