//! Current product-document loading, bootstrap, and replacement.
//!
//! The specification is owned exclusively by the planning agent. Users see a
//! rendered view in the center panel and never edit the file from the UI.

use std::path::Path;

use crate::artifacts::{SPEC_FILE, atomic_write, read_utf8_lossy, repo_artifact};

/// Fresh skeleton planted when a repository is first connected.
pub fn bootstrap_template(title: &str) -> String {
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut text = format!(
        "# {title} — Living Technical Specification\n\n\
         **Version:** 0.1. **Status:** Initial draft; intent and implementation unconfirmed.\n\n\
         **Authority:** Current planning specification, subject to confirmed project precedence.\n\n\
         **Origin / latest revision:** Packet bootstrap; no product decisions recorded.\n\n\
         **Maintenance.** The planning agent proposes coherent full revisions through accepted turns. \
         Packet validates and persists them; the UI is read-only. Git preserves history.\n\n"
    );
    let contents = [
        "Purpose, current behavior and goals are not established. Describe the project in chat to begin.",
        "Target users, their needs, desired outcomes and ownership require confirmation.",
        "Current behavior, in-scope capabilities, boundaries and deferred work require evidence and confirmation.",
        "Architecture, important data, runtime constraints and deployment assumptions require repository inspection.",
        "| ID | Decision | Basis | Status |\n| --- | --- | --- | --- |",
        "Quality expectations, risks and acceptance evidence are unspecified. Establish the relevant bar before claiming completion.",
    ];
    for (section, content) in crate::core::specification::SECTIONS.iter().zip(contents) {
        text.push_str(&format!("## {section}\n\n{content}\n\n"));
    }
    text
}

/// Read the specification, returning `Ok(None)` when the file does not exist.
pub fn load(repo_root: &Path) -> anyhow::Result<Option<String>> {
    if let Some(product) = crate::artifacts::product_docs::render_product(repo_root)? {
        return Ok(Some(product));
    }
    let path = repo_artifact(repo_root, SPEC_FILE);
    match read_utf8_lossy(&path) {
        Ok(t) => Ok(Some(t)),
        Err(_) if !path.exists() => Ok(None),
        Err(e) => Err(e),
    }
}

/// Ensure the current product document exists; returns true when created.
pub fn ensure(repo_root: &Path, title: &str) -> anyhow::Result<bool> {
    Ok(!crate::artifacts::migration::bootstrap_product(repo_root, title)?.is_empty())
}

/// Replace the whole specification (called from the apply step only).
pub fn write_full(repo_root: &Path, markdown: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        crate::artifacts::product_docs::load_modules(repo_root)?.is_none(),
        "The product specification is modular; update affected modules by logical ID"
    );
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
        crate::core::specification::validate_layout(&got).unwrap();
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
        let dir =
            std::env::temp_dir().join(format!("packet_test_{}_{}", prefix, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("mktemp");
        Temp { dir }
    }
}
