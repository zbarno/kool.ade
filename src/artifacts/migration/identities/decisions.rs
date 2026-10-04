use std::{collections::BTreeMap, path::Path};

use crate::{artifacts::layout::canonical, domain::ArtifactIdentity};

use super::super::plan::Plan as ArtifactPlan;
use super::{Change, change, files_under, preserve_markdown, register};

pub(super) fn build(
    repo: &Path,
    migration: &ArtifactPlan,
    task_uids: &BTreeMap<String, String>,
    seen: &mut BTreeMap<String, String>,
) -> anyhow::Result<Vec<Change>> {
    let files = files_under(repo, migration, canonical::DECISIONS)?;
    let mut max_id = files
        .values()
        .filter_map(|text| ArtifactIdentity::from_markdown(text).ok().flatten())
        .filter_map(|identity| {
            identity
                .display_id
                .strip_prefix("ADR-")?
                .parse::<u32>()
                .ok()
        })
        .max()
        .unwrap_or(0);
    let mut changes = Vec::new();
    let mut display_ids = BTreeMap::<String, String>::new();
    for (path, markdown) in files {
        if !path.ends_with(".md") {
            continue;
        }
        let Some(title) = markdown.lines().find_map(|line| line.strip_prefix("# ")) else {
            // Leave archived prose that is not an ADR document untouched.
            continue;
        };
        let current = ArtifactIdentity::from_markdown(&markdown)?;
        let task_uid = ticket_path(&markdown).and_then(|ticket| {
            task_uids.get(ticket).cloned().or_else(|| {
                crate::artifacts::migration::plan::relocated_ticket_path(ticket)
                    .and_then(|path| task_uids.get(&path).cloned())
            })
        });
        let suggested = if let Some(identity) = &current {
            identity.display_id.clone()
        } else {
            max_id += 1;
            format!("ADR-{max_id:03}")
        };
        let (contents, identity) =
            preserve_markdown(&markdown, &suggested, title.trim(), task_uid.as_deref())?;
        anyhow::ensure!(
            identity.display_id.starts_with("ADR-"),
            "Decision record identity at {path} must use an ADR display ID"
        );
        if let Some(previous) = display_ids.insert(identity.display_id.clone(), path.clone()) {
            anyhow::ensure!(
                previous == path,
                "Decision records {previous} and {path} share display ID {}; resolve the duplicate before connecting",
                identity.display_id
            );
        }
        register(seen, &identity, &format!("adr:{path}"))?;
        change(&mut changes, path, &markdown, contents);
    }
    Ok(changes)
}

fn ticket_path(markdown: &str) -> Option<&str> {
    markdown.lines().find_map(|line| {
        let ticket = line.strip_prefix("- Ticket: `")?;
        ticket.strip_suffix('`')
    })
}
