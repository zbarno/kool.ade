use std::{collections::BTreeMap, path::Path};

use crate::{artifacts::layout::canonical, core::workflow::TaskBatchRef, domain::ArtifactIdentity};

pub(super) fn group_files(
    files: BTreeMap<String, String>,
) -> anyhow::Result<BTreeMap<String, BTreeMap<String, String>>> {
    let prefix = format!("{}/", canonical::TASKS);
    let mut groups = BTreeMap::new();
    for (path, content) in files {
        let Some(rest) = path.strip_prefix(&prefix) else {
            continue;
        };
        let Some((directory, name)) = rest.split_once('/') else {
            continue;
        };
        if name.contains('/') || directory.is_empty() || name.is_empty() {
            continue;
        }
        let directory = format!("{}{directory}", prefix);
        validate_batch_path(&directory)?;
        groups
            .entry(directory)
            .or_insert_with(BTreeMap::new)
            .insert(name.to_owned(), content);
    }
    Ok(groups)
}

pub(super) fn validate_batch_path(directory: &str) -> anyhow::Result<()> {
    let prefix = format!("{}/", canonical::TASKS);
    let name = directory
        .strip_prefix(&prefix)
        .ok_or_else(|| anyhow::anyhow!("Task batch path is outside the Packet task root"))?;
    anyhow::ensure!(
        !name.is_empty()
            && !name.contains('/')
            && name.len() <= 120
            && name.chars().all(|c| c.is_alphanumeric() || c == '-'),
        "Invalid task batch directory {directory}"
    );
    Ok(())
}

pub(super) fn choose_batch_identity(
    workflow: Option<ArtifactIdentity>,
    progress: Option<ArtifactIdentity>,
    readme: Option<ArtifactIdentity>,
    title: String,
) -> anyhow::Result<ArtifactIdentity> {
    let mut chosen: Option<ArtifactIdentity> = None;
    for candidate in [workflow, progress, readme].into_iter().flatten() {
        if let Some(previous) = &chosen {
            anyhow::ensure!(
                previous.uid == candidate.uid && previous.display_id == candidate.display_id,
                "Task batch identity conflicts between workflow, progress checkpoint, and README"
            );
        } else {
            chosen = Some(candidate);
        }
    }
    let mut identity = chosen.unwrap_or_else(|| {
        let mut identity = ArtifactIdentity::new("pending", &title);
        identity.display_id = format!("BATCH-{}", identity.uid[..8].to_ascii_uppercase());
        identity
    });
    identity.title = if title.trim().is_empty() {
        identity.title
    } else {
        title
    };
    Ok(identity)
}

pub(super) fn batch_title(
    workflow: Option<&TaskBatchRef>,
    progress: &serde_json::Value,
    files: &BTreeMap<String, String>,
    directory: &str,
) -> String {
    progress
        .pointer("/brief/featureName")
        .and_then(serde_json::Value::as_str)
        .or_else(|| workflow.map(|batch| batch.feature.as_str()))
        .or_else(|| {
            files
                .get("README.md")
                .and_then(|text| text.lines().next())
                .map(|line| {
                    line.trim_start_matches("# ")
                        .split(" — task stories")
                        .next()
                        .unwrap_or(line)
                })
        })
        .unwrap_or_else(|| directory.rsplit('/').next().unwrap_or("Task batch"))
        .trim()
        .to_owned()
}

pub(super) fn feature_id_from_directory(directory: &str) -> Option<String> {
    let name = directory.rsplit('/').next()?;
    let id = if let Some(rest) = name.strip_prefix("CHG-") {
        let digits = rest.split_once('-')?.0;
        format!("CHG-{digits}")
    } else {
        name.split_once('-')?.0.to_owned()
    };
    crate::artifacts::product_docs::valid_feature_id(&id).then_some(id)
}

pub(super) fn attach_parent(
    mut identity: ArtifactIdentity,
    parent_uid: Option<&str>,
) -> anyhow::Result<ArtifactIdentity> {
    if let Some(parent_uid) = parent_uid {
        anyhow::ensure!(
            identity
                .parent_uid
                .as_deref()
                .is_none_or(|previous| previous == parent_uid),
            "Task batch is already linked to a different feature identity"
        );
        identity.parent_uid = Some(parent_uid.to_owned());
    }
    Ok(identity)
}

pub(super) fn markdown_with_identity(
    markdown: &str,
    identity: &ArtifactIdentity,
) -> anyhow::Result<String> {
    let seed = format!(
        "<!-- packet-artifact-id:v1 {} -->",
        serde_json::to_string(identity)?
    );
    let previous = if ArtifactIdentity::from_markdown(markdown)?.is_some() {
        markdown
    } else {
        &seed
    };
    ArtifactIdentity::preserve_markdown_with_parent(
        markdown,
        Some(previous),
        &identity.display_id,
        &identity.title,
        identity.parent_uid.as_deref(),
    )
}

pub(super) fn task_title<'a>(markdown: &'a str, filename: &'a str) -> (String, &'a str) {
    let heading = markdown
        .lines()
        .find_map(|line| line.strip_prefix("# "))
        .unwrap_or(filename);
    if let Some((display_id, title)) = heading.split_once(" — ") {
        (display_id.trim().to_owned(), title.trim())
    } else {
        let fallback = Path::new(filename)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or(filename);
        (fallback.to_owned(), heading.trim())
    }
}
