use super::*;

// Import dialog
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct DlgImport {
    pub paths: String,                    // one per line (files or folders)
    pub feedback: Option<(bool, String)>, // (ok, message)
}

impl DlgImport {
    pub fn new() -> Self {
        Self::default()
    }

    /// Stage the listed paths into Koolade imports, then checkpoint exactly
    /// those imported files without touching unrelated staged work.
    pub fn apply(&mut self, proj: &mut Project) -> Result<usize, AppError> {
        // Writer section: import writes + checkpoint share the index.
        let _guard = crate::core::writer_gate::acquire();
        let mut staged = 0usize;
        let mut authorized_paths = Vec::new();
        for line in self.paths.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let expanded = expand_tilde(line);
            let src = std::path::Path::new(&expanded);
            if !src.exists() {
                return Err(AppError::InvalidRepo {
                    path: line.to_string(),
                    detail: "file or folder does not exist".into(),
                });
            }
            let (doc, revision) = imports_io::import_into_store(
                &proj.state.planning_store,
                &proj.state.repo_root,
                src,
                &proj.state.baseline_planning_revision,
            )
            .map_err(|e| AppError::Io {
                op: format!("import {line}"),
                detail: e.to_string(),
            })?;
            proj.state.baseline_planning_revision = revision;
            let imports = proj
                .state
                .planning_store
                .git_path(crate::artifacts::planning_store::paths::IMPORTS);
            authorized_paths.push(format!("{imports}/{}", doc.stored_name));
            if let Some(companion) = doc.companion {
                authorized_paths.push(format!("{imports}/{companion}"));
            }
            staged += 1;
        }
        if staged > 0 {
            gitops::commit(
                &proj.state.planning_store.git_root(),
                "planner: import reference material",
                &authorized_paths,
            )
            .map_err(|e| {
                AppError::Other(format!("imports were staged but checkpoint failed: {e}"))
            })?;
            proj.refresh_git();
        }
        Ok(staged)
    }
}

// ---------------------------------------------------------------------------
