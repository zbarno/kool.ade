//! Current product-document loading, bootstrap, and replacement.
//!
//! The specification is owned exclusively by the planning agent. Users see a
//! rendered view in the center panel and never edit the file from the UI.

use crate::artifacts::planning_store::PlanningRoot;

/// Fresh skeleton planted when a repository is first connected.
#[cfg(test)]
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
    let store = repo_root.planning_store();
    if store.published_repository_root.is_some() {
        return Ok(load_published(&store)?.and_then(|snapshot| snapshot.markdown));
    }
    #[cfg(test)]
    return load_local(repo_root);
    #[cfg(not(test))]
    anyhow::bail!("published product documentation requires a primary code repository")
}

#[cfg(test)]
pub(crate) fn load_local<R: PlanningRoot + ?Sized>(
    repo_root: &R,
) -> anyhow::Result<Option<String>> {
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

pub(crate) struct PublishedSpecificationSnapshot {
    pub(crate) pin: crate::core::published_specs::PublishedSpecPin,
    pub(crate) markdown: Option<String>,
}

pub(crate) fn load_published(
    store: &crate::artifacts::planning_store::PlanningStore,
) -> anyhow::Result<Option<PublishedSpecificationSnapshot>> {
    let Some(code_root) = store.published_repository_root.as_deref() else {
        return Ok(None);
    };
    let reader =
        crate::core::published_specs::PublishedSpecReader::for_primary_repository(code_root)?;
    let pin = match &store.published_spec_pin {
        Some(pin) => pin.clone(),
        None => reader.pin()?,
    };
    let markdown = crate::artifacts::product_docs::render_published_product(&reader, &pin)?;
    Ok(Some(PublishedSpecificationSnapshot { pin, markdown }))
}

/// Ensure the current product document exists; returns true when created.
#[cfg(test)]
pub fn ensure<R: PlanningRoot + ?Sized>(repo_root: &R, title: &str) -> anyhow::Result<bool> {
    let store_root = repo_root.planning_layout().root().to_path_buf();
    Ok(
        !crate::artifacts::product_bootstrap::bootstrap_product_with_store(
            &store_root,
            &repo_root.planning_store(),
            title,
        )?
        .is_empty(),
    )
}

/// Replace the whole specification (called from the apply step only).
#[cfg(test)]
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
        assert_eq!(load(tmp.path()).ok(), Some(None));
        ensure(tmp.path(), "Demo Proj").ok();
        let got = load(tmp.path()).unwrap().unwrap();
        assert!(got.starts_with("# Demo Proj"));
        crate::core::specification::validate_layout(&got).unwrap();
    }

    #[test]
    fn published_spec_rendering_ignores_private_product_drafts() {
        let tmp = tempfile_like("pinned_source");
        let code = tmp.path().join("code");
        let local = tmp.path().join("local");
        fs::create_dir_all(code.join("docs/koolade/product")).unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q", "-b", "main"])
                .current_dir(&code)
                .status()
                .unwrap()
                .success()
        );
        for (key, value) in [
            ("user.name", "Koolade Test"),
            ("user.email", "koolade-test@example.invalid"),
        ] {
            assert!(
                std::process::Command::new("git")
                    .args(["config", key, value])
                    .current_dir(&code)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        fs::write(
            code.join("docs/koolade/product/overview.md"),
            "# Product\n\nPublished source truth.\n",
        )
        .unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["add", "."])
                .current_dir(&code)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            std::process::Command::new("git")
                .args(["commit", "-q", "-m", "published product spec"])
                .current_dir(&code)
                .status()
                .unwrap()
                .success()
        );

        let store = crate::artifacts::planning_store::PlanningStore::for_code_repository(
            uuid::Uuid::new_v4(),
            &local,
            &code,
        );
        crate::artifacts::product_bootstrap::bootstrap_product_with_store(&local, &store, "Draft")
            .unwrap();
        let local_overview = local.join("planning/product/overview.md");
        fs::write(&local_overview, "# Overview\n\nPrivate draft only.\n").unwrap();

        let snapshot = load_published(&store).unwrap().unwrap();
        let published = snapshot.markdown.unwrap();
        assert!(published.contains("Published source truth."));
        assert!(!published.contains("Private draft only."));
        let loaded = load(&store).unwrap().unwrap();
        assert!(loaded.contains("Published source truth."));
        assert!(!loaded.contains("Private draft only."));
        assert!(
            crate::artifacts::product_docs::render_product(&store)
                .unwrap()
                .unwrap()
                .contains("Published source truth.")
        );
        assert!(
            !crate::artifacts::product_docs::render_product(&store)
                .unwrap()
                .unwrap()
                .contains("Private draft only.")
        );
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
