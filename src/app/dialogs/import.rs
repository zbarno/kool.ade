use super::*;

fn expand_tilde(raw: &str) -> String {
    if let (Some(rest), Some(home)) = (raw.strip_prefix("~/"), std::env::var("HOME").ok()) {
        return format!("{home}/{rest}");
    }
    raw.to_owned()
}

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

    /// Copy the listed reference material into private operational storage.
    pub fn apply(&mut self, proj: &mut Project) -> Result<usize, AppError> {
        // Writer section: all imports are written into private local state.
        let _guard = crate::core::writer_gate::acquire();
        let mut staged = 0usize;
        for line in self.paths.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let expanded = expand_tilde(line);
            let src = std::path::Path::new(&expanded);
            if !src.exists() {
                return Err(AppError::InvalidRepo {
                    path: line.to_string(),
                    detail: "file or folder does not exist".into(),
                });
            }
            let (_doc, revision) = imports_io::import_into_store(
                &proj.state.planning_store,
                src,
                &proj.state.baseline_planning_revision,
            )
            .map_err(|e| AppError::Io {
                op: format!("import {line}"),
                detail: e.to_string(),
            })?;
            proj.state.baseline_planning_revision = revision;
            staged += 1;
        }
        Ok(staged)
    }
}

// ---------------------------------------------------------------------------
