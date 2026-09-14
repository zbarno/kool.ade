//! One logical product specification stored as independently replaceable modules.
//! The legacy file is parsed as evidence and never altered until a complete
//! replacement directory has been built and installed.
use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};
use std::path::{Path, PathBuf};

use crate::core::specification::SECTIONS;

pub const PRODUCT_DIR: &str = "planning/product";
pub const INDEX: &str = "planning/product/index.md";
pub const LEGACY_ARCHIVE: &str = "planning/archive/specification-pre-modules.md";
pub const MODULES: [&str; 13] = [
    "01-vision.md",
    "02-scope.md",
    "03-actors-and-roles.md",
    "04-feature-inventory.md",
    "05-functional-requirements.md",
    "06-non-functional-requirements.md",
    "07-data-model.md",
    "08-architecture.md",
    "09-environment.md",
    "10-decisions.md",
    "11-risks.md",
    "12-acceptance.md",
    "13-source-map.md",
];

fn regular(path: &Path) -> anyhow::Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) => {
            anyhow::ensure!(
                meta.is_file() && !meta.file_type().is_symlink(),
                "{} is not a regular file",
                path.display()
            );
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}
fn real_dir(path: &Path) -> anyhow::Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) => {
            anyhow::ensure!(
                meta.is_dir() && !meta.file_type().is_symlink(),
                "{} is not a real directory",
                path.display()
            );
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}
fn tracked(repo: &Path, relative: &str) -> bool {
    std::process::Command::new("git")
        .args(["ls-files", "--error-unmatch", "--", relative])
        .current_dir(repo)
        .output()
        .is_ok_and(|output| output.status.success())
}

pub fn module_path(repo: &Path, number: usize) -> anyhow::Result<PathBuf> {
    anyhow::ensure!((1..=13).contains(&number), "Invalid product module number");
    Ok(repo.join(PRODUCT_DIR).join(MODULES[number - 1]))
}

/// A logical document ID never becomes an agent-selected path.
pub fn next_feature_id(repo: &Path) -> String {
    let mut maximum = 0u32;
    if let Ok(entries) = std::fs::read_dir(repo.join("planning/features")) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if let Some(n) = name.get(4..7).and_then(|digits| digits.parse::<u32>().ok()) {
                    if name.starts_with("CHG-") {
                        maximum = maximum.max(n);
                    }
                }
            }
        }
    }
    if let Ok(output) = std::process::Command::new("git")
        .args([
            "log",
            "--all",
            "--name-only",
            "--pretty=format:",
            "--",
            "planning/features",
        ])
        .current_dir(repo)
        .output()
    {
        if output.status.success() {
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                if let Some(name) = line.strip_prefix("planning/features/CHG-") {
                    if let Some(n) = name.get(..3).and_then(|digits| digits.parse::<u32>().ok()) {
                        maximum = maximum.max(n);
                    }
                }
            }
        }
    }
    format!("CHG-{:03}", maximum + 1)
}

pub fn document_path_for_update(repo: &Path, id: &str, content: &str) -> anyhow::Result<PathBuf> {
    if let Ok(existing) = document_path(repo, id) {
        return Ok(existing);
    }
    let feature_id = id
        .strip_prefix("feature:")
        .ok_or_else(|| anyhow::anyhow!("Unknown logical document ID"))?;
    anyhow::ensure!(
        feature_id == next_feature_id(repo),
        "New feature ID must be application-assigned next ID"
    );
    crate::core::specification::validate_feature(feature_id, content)?;
    let title = content
        .lines()
        .find_map(|line| line.strip_prefix(&format!("# {feature_id}: ")))
        .ok_or_else(|| anyhow::anyhow!("Feature title missing"))?;
    let slug = crate::artifacts::task_docs::slug(title);
    let root = repo.join("planning/features");
    let _ = real_dir(&root)?;
    let dir = root.join(format!("{feature_id}-{slug}"));
    anyhow::ensure!(!dir.exists(), "Feature directory already exists");
    Ok(dir.join("specification.md"))
}

pub fn refreshed_index(repo: &Path, updates: &[(String, String)]) -> anyhow::Result<String> {
    let index = std::fs::read_to_string(repo.join(INDEX))?;
    let marker = "## Active features";
    let prefix = index
        .split_once(marker)
        .map(|(prefix, _)| prefix)
        .ok_or_else(|| anyhow::anyhow!("Product index lacks active feature manifest"))?;
    let mut entries = active_features(repo);
    for (id, content) in updates {
        if !id.starts_with("feature:") {
            continue;
        }
        let path = document_path_for_update(repo, id, content)?;
        let name = path
            .parent()
            .and_then(|dir| dir.file_name())
            .and_then(|name| name.to_str())
            .ok_or_else(|| anyhow::anyhow!("Invalid feature path"))?;
        entries.retain(|entry| entry.get(..7) != name.get(..7));
        if !content.contains("**Status:** Implemented")
            && !content.contains("**Status:** Abandoned")
        {
            entries.push(name.to_string());
        }
    }
    entries.sort();
    entries.dedup();
    let mut result = format!("{}{}\n\n", prefix, marker);
    if entries.is_empty() {
        result.push_str("None.\n");
    } else {
        for name in entries {
            result.push_str(&format!(
                "- [`{name}`](../features/{name}/specification.md)\n"
            ));
        }
    }
    Ok(result)
}

pub fn preserved_ids(old: &str, new: &str) -> anyhow::Result<()> {
    fn ids(text: &str) -> std::collections::BTreeSet<String> {
        text.split(|c: char| !c.is_ascii_alphanumeric() && c != '-')
            .filter(|token| {
                let (prefix, digits) = token.rsplit_once('-').unwrap_or(("", ""));
                matches!(prefix, "G" | "F" | "FR" | "NFR" | "D" | "CLR")
                    && !digits.is_empty()
                    && digits.bytes().all(|b| b.is_ascii_digit())
            })
            .map(str::to_owned)
            .collect()
    }
    let missing = ids(old).difference(&ids(new)).cloned().collect::<Vec<_>>();
    anyhow::ensure!(
        missing.is_empty(),
        "Stable identifiers removed: {}",
        missing.join(", ")
    );
    Ok(())
}

pub fn document_path(repo: &Path, id: &str) -> anyhow::Result<PathBuf> {
    if id == "product:index" {
        return Ok(repo.join(INDEX));
    }
    if let Some(name) = id.strip_prefix("product:") {
        if let Some(n) = MODULES
            .iter()
            .position(|m| m.strip_suffix(".md") == Some(name))
        {
            return module_path(repo, n + 1);
        }
    }
    if let Some(id) = id.strip_prefix("feature:") {
        anyhow::ensure!(
            id.len() == 7 && id.starts_with("CHG-") && id[4..].bytes().all(|b| b.is_ascii_digit()),
            "Invalid feature document ID"
        );
        let root = repo.join("planning/features");
        anyhow::ensure!(real_dir(&root)?, "Feature directory does not exist");
        let mut matches = std::fs::read_dir(&root)?
            .map(|entry| entry.map(|e| e.path()))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|p| {
                p.file_name()
                    .and_then(|s| s.to_str())
                    .is_some_and(|name| name.starts_with(&format!("{id}-")))
            })
            .collect::<Vec<_>>();
        anyhow::ensure!(
            matches.len() == 1,
            "Feature ID must identify exactly one directory"
        );
        let dir = matches.remove(0);
        anyhow::ensure!(real_dir(&dir)?, "Feature directory is not a real directory");
        return Ok(dir.join("specification.md"));
    }
    anyhow::bail!("Unknown logical document ID: {id}")
}

pub fn validate_module(number: usize, text: &str) -> anyhow::Result<()> {
    anyhow::ensure!((1..=13).contains(&number), "Invalid product module number");
    let heads = headings(text);
    anyhow::ensure!(
        heads.iter().all(|(level, _)| *level != HeadingLevel::H1),
        "Product modules must not contain an H1"
    );
    let h2 = heads
        .iter()
        .filter(|(level, _)| *level == HeadingLevel::H2)
        .map(|(_, title)| title.as_str())
        .collect::<Vec<_>>();
    anyhow::ensure!(
        h2.len() == 1 && h2[0].starts_with(&format!("{number}. ")),
        "Product module {number} requires exactly one matching numbered H2"
    );
    anyhow::ensure!(!text.trim().is_empty(), "Product module must not be blank");
    Ok(())
}
fn headings(text: &str) -> Vec<(HeadingLevel, String)> {
    let mut out = Vec::new();
    let mut active = None;
    for event in Parser::new(text) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => active = Some((level, String::new())),
            Event::Text(value) | Event::Code(value) => {
                if let Some((_, title)) = &mut active {
                    title.push_str(&value);
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(h) = active.take() {
                    out.push(h);
                }
            }
            _ => {}
        }
    }
    out
}

pub fn split_legacy(text: &str) -> anyhow::Result<[String; 13]> {
    let mut starts = Vec::new();
    let mut active = None;
    for (event, range) in Parser::new(text).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading {
                level: HeadingLevel::H2,
                ..
            }) => active = Some((range.start, String::new())),
            Event::Text(value) | Event::Code(value) => {
                if let Some((_, title)) = &mut active {
                    title.push_str(&value);
                }
            }
            Event::End(TagEnd::Heading(HeadingLevel::H2)) => {
                if let Some(h) = active.take() {
                    starts.push(h);
                }
            }
            _ => {}
        }
    }
    anyhow::ensure!(
        starts.len() == 13,
        "Legacy specification must have exactly thirteen top-level sections"
    );
    for (n, (_, title)) in starts.iter().enumerate() {
        anyhow::ensure!(
            title.starts_with(&format!("{}. ", n + 1)),
            "Legacy section {} is missing or out of order",
            n + 1
        );
    }
    let parts = (0..13)
        .map(|i| {
            let start = starts[i].0;
            let end = starts.get(i + 1).map_or(text.len(), |next| next.0);
            text[start..end].trim_end().to_string() + "\n"
        })
        .collect::<Vec<_>>();
    Ok(parts.try_into().expect("thirteen sections"))
}

pub fn load_modules(repo: &Path) -> anyhow::Result<Option<[String; 13]>> {
    let root = repo.join(PRODUCT_DIR);
    if !real_dir(&root)? {
        return Ok(None);
    }
    anyhow::ensure!(regular(&root.join("index.md"))?, "Product index is missing");
    let mut parts = Vec::with_capacity(13);
    for (n, name) in MODULES.iter().enumerate() {
        let path = root.join(name);
        anyhow::ensure!(regular(&path)?, "Product module {} is missing", name);
        let text = std::fs::read_to_string(&path)?;
        validate_module(n + 1, &text)?;
        parts.push(text);
    }
    Ok(Some(parts.try_into().expect("thirteen modules")))
}

pub fn render_product(repo: &Path) -> anyhow::Result<Option<String>> {
    let Some(modules) = load_modules(repo)? else {
        return Ok(None);
    };
    let index = std::fs::read_to_string(repo.join(INDEX))?;
    let title = index
        .lines()
        .find_map(|line| line.strip_prefix("# "))
        .unwrap_or("Product");
    let mut text = format!("# {title}\n\n");
    for module in modules {
        text.push_str(&module);
        text.push('\n');
    }
    Ok(Some(text))
}

pub fn active_feature(repo: &Path) -> Option<(String, String)> {
    for name in active_features(repo) {
        let id = name.get(..7)?.to_string();
        let body = std::fs::read_to_string(
            repo.join("planning/features")
                .join(&name)
                .join("specification.md"),
        )
        .ok()?;
        return Some((id, body));
    }
    None
}

fn active_features(repo: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(repo.join("planning/features")) else {
        return Vec::new();
    };
    let mut entries = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let path = entry.path().join("specification.md");
            let body = std::fs::read_to_string(path).ok()?;
            if body.contains("**Status:** Implemented") || body.contains("**Status:** Abandoned") {
                return None;
            }
            Some(name)
        })
        .collect::<Vec<_>>();
    entries.sort();
    entries
}

/// Install a complete module directory with a single rename. The old file
/// remains available until that rename succeeds, then moves to the archive.
/// A second call is safe after a crash at either boundary.
pub fn migrate(repo: &Path, legacy: &str) -> anyhow::Result<Vec<String>> {
    let planning = repo.join("planning");
    anyhow::ensure!(
        real_dir(&planning)?,
        "Planning directory is not a real directory"
    );
    let product = repo.join(PRODUCT_DIR);
    let mut changes = Vec::new();
    if !real_dir(&product)? {
        let parts = split_legacy(legacy)?;
        let staged = planning.join(format!(
            ".product-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir(&staged)?;
        let result = (|| -> anyhow::Result<()> {
            let title = legacy
                .lines()
                .find_map(|l| l.strip_prefix("# "))
                .unwrap_or("Product")
                .trim_end_matches(" — Living Technical Specification");
            let mut index = format!(
                "# {title} — Living Technical Specification\n\nStatus: migrated current product specification.\n\nThe modules below are the current product authority. The pre-migration source is archived and retained in git history.\n\n## Modules\n\n"
            );
            for (n, (name, body)) in MODULES.iter().zip(parts).enumerate() {
                validate_module(n + 1, &body)?;
                std::fs::write(staged.join(name), body)?;
                index.push_str(&format!("- [`{name}`]({name}) — {}\n", SECTIONS[n]));
            }
            let features = active_features(repo);
            index.push_str("\n## Active features\n\n");
            if features.is_empty() {
                index.push_str("None.\n");
            } else {
                for feature in features {
                    index.push_str(&format!(
                        "- [`{feature}`](../features/{feature}/specification.md)\n"
                    ));
                }
            }
            std::fs::write(staged.join("index.md"), index)?;
            std::fs::rename(&staged, &product)?;
            Ok(())
        })();
        if staged.exists() {
            let _ = std::fs::remove_dir_all(&staged);
        }
        result?;
        changes.push(INDEX.to_string());
        changes.extend(MODULES.iter().map(|name| format!("{PRODUCT_DIR}/{name}")));
    } else {
        load_modules(repo)?;
        if repo.join(".git").exists() && !tracked(repo, INDEX) {
            changes.push(INDEX.to_string());
            changes.extend(MODULES.iter().map(|name| format!("{PRODUCT_DIR}/{name}")));
        }
    }
    let old = repo.join("planning/specification.md");
    let archive_dir = repo.join("planning/archive");
    if regular(&old)? {
        if !real_dir(&archive_dir)? {
            std::fs::create_dir(&archive_dir)?;
        }
        let archive = repo.join(LEGACY_ARCHIVE);
        anyhow::ensure!(
            !regular(&archive)?,
            "Legacy archive already exists while old specification remains"
        );
        std::fs::rename(old, archive)?;
        changes.push("planning/specification.md".into());
        changes.push(LEGACY_ARCHIVE.into());
    }
    if repo.join(".git").exists()
        && regular(&repo.join(LEGACY_ARCHIVE))?
        && !tracked(repo, LEGACY_ARCHIVE)
    {
        if !changes.iter().any(|path| path == LEGACY_ARCHIVE) {
            changes.push(LEGACY_ARCHIVE.into());
        }
        if tracked(repo, "planning/specification.md")
            && !changes
                .iter()
                .any(|path| path == "planning/specification.md")
        {
            changes.push("planning/specification.md".into());
        }
    }
    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn migration_preserves_all_thirteen_sections_and_stable_ids() {
        let root =
            std::env::temp_dir().join(format!("packet_product_migrate_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("planning")).unwrap();
        let source = crate::artifacts::spec_doc::bootstrap_template("Demo");
        std::fs::write(root.join("planning/specification.md"), &source).unwrap();
        let paths = migrate(&root, &source).unwrap();
        assert_eq!(paths.len(), 16);
        assert!(migrate(&root, &source).unwrap().is_empty());
        let parts = load_modules(&root).unwrap().unwrap();
        assert!(parts[0].contains("Purpose"));
        assert_eq!(parts.len(), 13);
        assert!(root.join(LEGACY_ARCHIVE).is_file());
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn logical_ids_cannot_escape_allowlisted_paths() {
        let root = Path::new("/repo");
        assert_eq!(
            document_path(root, "product:08-architecture").unwrap(),
            root.join(PRODUCT_DIR).join("08-architecture.md")
        );
        for id in [
            "product:../../etc/passwd",
            "feature:CHG-001/../../etc",
            "product:14-not-real",
        ] {
            assert!(document_path(root, id).is_err());
        }
    }
    #[test]
    fn restart_after_uncommitted_migration_still_reports_all_paths() {
        let root = std::env::temp_dir().join(format!(
            "packet_product_restart_{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::create_dir_all(root.join("planning")).unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        let source = crate::artifacts::spec_doc::bootstrap_template("Demo");
        std::fs::write(root.join("planning/specification.md"), &source).unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["add", "planning/specification.md"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            std::process::Command::new("git")
                .args([
                    "-c",
                    "user.name=Fixture",
                    "-c",
                    "user.email=fixture@example.test",
                    "commit",
                    "-qm",
                    "seed"
                ])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        migrate(&root, &source).unwrap();
        let retry = migrate(&root, &source).unwrap();
        assert_eq!(retry.len(), 16);
        assert!(retry.contains(&INDEX.to_string()));
        assert!(retry.contains(&"planning/specification.md".to_string()));
        let _ = std::fs::remove_dir_all(root);
    }
}
