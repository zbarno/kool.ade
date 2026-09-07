//! `planning/specification.md` — load, bootstrap, replace (SPECIFICATION.md §4).
//!
//! The specification is owned exclusively by the planning agent. Users see a
//! rendered view in the center panel and never edit the file from the UI.

use std::path::Path;

use crate::artifacts::{atomic_write, read_utf8_lossy, repo_artifact, SPEC_FILE};

/// Fresh skeleton planted when a repository is first connected.
pub fn bootstrap_template(title: &str) -> String {
    format!(
        "# {title}\n\n\
         ## Overview\n\n\
         _Not established yet — describe the project in the chat to begin._\n"
    )
}

/// Read the specification, returning `Ok(None)` when the file does not exist.
pub fn load(repo_root: &Path) -> anyhow::Result<Option<String>> {
    let path = repo_artifact(repo_root, SPEC_FILE);
    match read_utf8_lossy(&path) {
        Ok(t) => Ok(Some(t)),
        Err(_) if path.exists() == false => Ok(None),
        Err(e) => Err(e),
    }
}

/// Ensure `planning/specification.md` exists; returns true when it was created.
pub fn ensure(repo_root: &Path, title: &str) -> anyhow::Result<bool> {
    let path = repo_artifact(repo_root, SPEC_FILE);
    if path.exists() {
        return Ok(false);
    }
    atomic_write(&path, &bootstrap_template(title))?;
    Ok(true)
}

/// Replace the whole specification (called from the apply step only).
pub fn write_full(repo_root: &Path, markdown: &str) -> anyhow::Result<()> {
    atomic_write(&repo_artifact(repo_root, SPEC_FILE), markdown)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn load_returns_none_when_absent_and_reads_when_present() {
        let tmp = tempfile_like("spec_doc");
        fs::create_dir_all(tmp.path()).ok();
        assert_eq!(load(tmp.path()).ok(), Some(None));
        ensure(tmp.path(), "Demo Proj").ok();
        let got = load(tmp.path()).unwrap().unwrap();
        assert!(got.starts_with("# Demo Proj"));
    }

    /// Minimal tempdir without pulling in the `tempfile` crate.
    struct Temp {
        dir: std::path::PathBuf,
    }
    impl Temp {
        fn path(&self) -> &std::path::Path {
            &self.dir
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }
    fn tempfile_like(prefix: &str) -> Temp {
        let dir = std::env::temp_dir().join(format!(
            "packet_test_{}_{}",
            prefix,
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("mktemp");
        Temp { dir }
    }
}
