//! Current product-document loading, bootstrap, and replacement.
//!
//! The specification is owned exclusively by the planning agent. Users see a
//! rendered view in the center panel and never edit the file from the UI.

use crate::artifacts::planning_store::PlanningRoot;

/// Fresh skeleton planted when a repository is first connected.
pub fn bootstrap_template(title: &str) -> String {
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut text = format!(
        "# {title} — Living Technical Specification\n\n\
         **Version:** 0.1. **Status:** Initial draft; intent and implementation unconfirmed.\n\n\
         **Authority:** Current planning specification, subject to confirmed project precedence.\n\n\
         **Origin / latest revision:** Kool.ad/e bootstrap; no product decisions recorded.\n\n\
         **Maintenance.** The planning agent proposes coherent full revisions through accepted turns. \
         Kool.ad/e validates and persists them; the UI is read-only. Git preserves history.\n\n"
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
pub fn load<R: PlanningRoot + ?Sized>(repo_root: &R) -> anyhow::Result<Option<String>> {
    if let Some(product) = crate::artifacts::product_docs::render_product(repo_root)? {
        return Ok(Some(product));
    }
    match repo_root.read_planning(crate::artifacts::planning_store::paths::PRODUCT_INDEX) {
        Ok(bytes) => Ok(Some(String::from_utf8(bytes)?)),
        Err(crate::artifacts::planning_store::StoreError::Io { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound =>
        {
            Ok(None)
        }
        Err(error) => Err(error.into()),
    }
}

/// Ensure the current product document exists; returns true when created.
pub fn ensure<R: PlanningRoot + ?Sized>(repo_root: &R, title: &str) -> anyhow::Result<bool> {
    let code_root = repo_root.code_repository_root().ok_or_else(|| {
        anyhow::anyhow!("Managed planning stores use managed workspace bootstrap")
    })?;
    Ok(!crate::artifacts::migration::bootstrap_product_with_store(
        &code_root,
        &repo_root.planning_store(),
        title,
    )?
    .is_empty())
}

/// Replace the whole specification (called from the apply step only).
pub fn write_full<R: PlanningRoot + ?Sized>(repo_root: &R, markdown: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        crate::artifacts::product_docs::load_modules(repo_root)?.is_none(),
        "The product specification is modular; update affected modules by logical ID"
    );
    repo_root.write_planning(
        crate::artifacts::planning_store::paths::PRODUCT_INDEX,
        markdown.as_bytes(),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn load_returns_none_when_absent_and_reads_when_present() {
        let tmp = tempfile_like("spec_doc");
        fs::create_dir_all(tmp.path()).ok();
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q"])
                .current_dir(tmp.path())
                .status()
                .unwrap()
                .success()
        );
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
            std::env::temp_dir().join(format!("koolade_test_{}_{}", prefix, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("mktemp");
        Temp { dir }
    }
}
