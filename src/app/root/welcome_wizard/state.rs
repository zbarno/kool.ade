/// The first-party journey starters offered on the welcome landing.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::app::root) enum WelcomeEntry {
    /// Clone code and its paired planning repository into Kool.ad/e-managed locations.
    CloneRepository,
    /// Nothing exists yet; both repositories are created from scratch.
    BlankProject,
}

impl WelcomeEntry {
    pub(super) fn title(self) -> &'static str {
        match self {
            Self::CloneRepository => "Clone a code repository and its planning repository",
            Self::BlankProject => "Start a brand-new project",
        }
    }
}

/// Which wizard field the native folder picker should fill.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::app::root) enum WizField {
    /// BlankProject entry: the parent folder for the new code checkout.
    Parent,
}

/// Draft state for ONE active welcome wizard. Closing a draft discards it;
/// nothing touches the disk until the confirm step commits.
pub(in crate::app::root) struct WelcomeWizard {
    pub(super) entry: WelcomeEntry,
    /// 0 = gather inputs, 1 = confirm the planned side effects.
    pub(super) step: u8,
    pub(super) project_name: String,
    /// CloneRepository: the repository URL to download (cloned into
    /// `~/kool-ade-workspaces/<repo>` by the shared clone engine).
    pub(super) url: String,
    /// BlankProject: parent folder for the new code checkout.
    pub(super) parent: String,
    pub(super) error: Option<String>,
}

impl WelcomeWizard {
    pub(super) fn new(entry: WelcomeEntry) -> Self {
        Self {
            entry,
            step: 0,
            project_name: String::new(),
            url: String::new(),
            parent: String::new(),
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

    /// Effective project name: the typed title, else the repository segment
    /// for a clone. Titles are cosmetic; the derived value only fills the field.
    pub(super) fn effective_project_name(&self) -> String {
        let trimmed = self.project_name.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
        match self.entry {
            WelcomeEntry::CloneRepository => crate::app::welcome::parse_github_url(self.url.trim())
                .map(|target| target.repo)
                .unwrap_or_default(),
            WelcomeEntry::BlankProject => String::new(),
        }
    }

    /// Code folder name where the code repository will live after commit.
    pub(super) fn code_folder_name(&self) -> String {
        match self.entry {
            WelcomeEntry::CloneRepository => crate::app::welcome::parse_github_url(self.url.trim())
                .map(|target| target.repo)
                .unwrap_or_default(),
            WelcomeEntry::BlankProject => {
                crate::app::root::KooladeApp::project_title_to_folder_name(
                    self.effective_project_name().as_str(),
                )
            }
        }
    }

    /// Planning repository name is fixed by the code repository name.
    pub(super) fn effective_planning_repository_name(&self) -> String {
        format!("{}.packet", self.code_folder_name())
    }

    /// Human-readable list of the effects confirmed by the commit step.
    pub(super) fn disk_plan_lines(&self) -> Vec<String> {
        let planning_repository = self.effective_planning_repository_name();
        match self.entry {
            WelcomeEntry::CloneRepository => {
                let name = self.code_folder_name();
                let code_repository = crate::app::welcome::parse_github_url(self.url.trim())
                    .map(|target| format!("{}/{}", target.owner, target.repo))
                    .unwrap_or_else(|_| name.clone());
                let planning_repository = crate::app::welcome::parse_github_url(self.url.trim())
                    .map(|target| format!("{}/{}.packet", target.owner, target.repo))
                    .unwrap_or(planning_repository);
                let shown = match self.clone_destination() {
                    Some(dest) => dest.display().to_string(),
                    None => format!("$HOME/kool-ade-workspaces/{name}"),
                };
                vec![
                    format!("Clones GitHub repository “{code_repository}” into “{shown}”."),
                    format!(
                        "Opens the separate GitHub planning repository “{planning_repository}” if it exists, or creates it with the code repository's visibility."
                    ),
                    "The planning repository has its own branch and Git history, separate from code branches."
                        .to_string(),
                    "Keeps planning files in Kool.ad/e's managed planning-repository location."
                        .to_string(),
                    "Registers the code clone and opens the planning board.".to_string(),
                ]
            }
            WelcomeEntry::BlankProject => {
                let name = self.code_folder_name();
                let parent = self.parent.trim();
                vec![
                    format!(
                        "Creates a fresh code repository “{parent}/{name}” (one placeholder README commit)."
                    ),
                    format!(
                        "Creates the separate planning repository “{planning_repository}” in Kool.ad/e's managed workspace area."
                    ),
                    "Registers the code repository in the planning repository.".to_string(),
                    "Opens the planning board for this project.".to_string(),
                ]
            }
        }
    }
}

#[cfg(test)]
mod tests;
