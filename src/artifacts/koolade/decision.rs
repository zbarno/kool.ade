//! Compact ADRs created from user-approved durable planning decisions.
#[cfg(test)]
use crate::artifacts::layout::ArtifactLayout;
use crate::artifacts::planning_store::PlanningRoot;
use crate::domain::{AdrAssessment, ArtifactIdentity, Authority, DecisionOption, OpenItem};
use std::path::{Path, PathBuf};

/// Prepare an immutable ADR artifact for the planning transaction.
///
/// Implementation reports and command evidence remain under the separate
/// implementation workspace; this document records only an approved decision.
pub(crate) fn prepare_decision_record<R: PlanningRoot + ?Sized>(
    repo: &R,
    item: &OpenItem,
) -> anyhow::Result<Option<(String, String)>> {
    let Some(brief) = item.decision_brief.as_ref() else {
        return Ok(None);
    };
    let Some(assessment) = brief.adr_assessment.as_ref() else {
        return Ok(None);
    };
    if !assessment.create {
        return Ok(None);
    }
    anyhow::ensure!(
        item.authority == Authority::Review,
        "Only an approved Review decision can create an ADR"
    );
    let feature_id = item
        .feature_id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Material decision has no related feature"))?;
    let recommendation = brief
        .recommendation
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Material decision has no approved recommendation"))?;
    let chosen = brief
        .options
        .iter()
        .find(|option| option.id == recommendation.option_id)
        .ok_or_else(|| anyhow::anyhow!("Approved decision option is missing"))?;

    let store = repo.planning_store();
    let layout = repo.planning_layout();
    let dir = layout.decisions_root();
    ensure_path_has_no_symlinks(layout.root(), &dir)?;
    let title = assessment.title.trim();
    if let Some((path, previous)) = matching_decision(repo, &dir, &item.id)? {
        let identity = ArtifactIdentity::from_markdown(&previous)?
            .ok_or_else(|| anyhow::anyhow!("Existing ADR has no Koolade identity"))?;
        let expected = render(
            &identity.display_id,
            item,
            feature_id,
            assessment,
            chosen,
            &recommendation.rationale,
        );
        anyhow::ensure!(
            stable_visible(&previous) == stable_visible(&expected),
            "An immutable ADR already exists for decision {}",
            item.id
        );
        return Ok(Some((relative_path(&store, &path)?, previous)));
    }

    let identity = super::identity::new_adr_identity(repo, &dir, title, item.uid.as_deref());
    let body = render(
        &identity.display_id,
        item,
        feature_id,
        assessment,
        chosen,
        &recommendation.rationale,
    );
    let content = super::identity::embed(&body, None, &identity)?;
    let file_name = format!(
        "{}-{}.md",
        identity.display_id,
        crate::artifacts::task_docs::slug(title)
    );
    let path = layout
        .decision_record(&file_name)
        .ok_or_else(|| anyhow::anyhow!("Generated ADR filename is unsafe"))?;
    ensure_path_has_no_symlinks(layout.root(), &path)?;
    anyhow::ensure!(
        !path.exists(),
        "ADR path already exists without matching decision identity: {}",
        path.display()
    );
    Ok(Some((relative_path(&store, &path)?, content)))
}

fn render(
    display_id: &str,
    item: &OpenItem,
    feature_id: &str,
    assessment: &AdrAssessment,
    selected: &DecisionOption,
    recommendation_rationale: &str,
) -> String {
    let brief = item
        .decision_brief
        .as_ref()
        .expect("validated decision brief");
    let mut body = format!(
        "# {display_id}: {}\n\nStatus: Accepted\nDate: {}\nRelated change: `{feature_id}`\nDecision item: `{}`\n\n## Context\n\n{}\n\n{}\n",
        assessment.title.trim(),
        chrono::Utc::now().format("%Y-%m-%d"),
        item.id,
        item.reason.trim(),
        brief.why_now.trim()
    );
    if !brief.evidence.is_empty() {
        append_list(&mut body, "Evidence considered", &brief.evidence);
    }
    body.push_str(&format!(
        "\n## Decision\n\n**{}** — {}\n\n## Alternatives considered\n",
        selected.label.trim(),
        selected.summary.trim()
    ));
    for option in &brief.options {
        if option.id == selected.id {
            continue;
        }
        body.push_str(&format!(
            "\n- **{}** — {}",
            option.label.trim(),
            option.summary.trim()
        ));
        append_inline(&mut body, "Consequence", &option.consequences);
        append_inline(&mut body, "Costs", &option.costs);
        append_inline(&mut body, "Risks", &option.risks);
        body.push('\n');
    }
    body.push_str(&format!(
        "\n## Why\n\n{}\n",
        recommendation_rationale.trim()
    ));
    body.push_str("\n## Consequences\n");
    append_labeled_list(&mut body, "Benefits", &selected.benefits);
    append_labeled_list(&mut body, "Costs", &selected.costs);
    append_labeled_list(&mut body, "Risks", &selected.risks);
    append_labeled_list(&mut body, "Outcome", &selected.consequences);
    append_labeled_list(&mut body, "Overall ramifications", &brief.ramifications);
    body.push_str("\n## Revisit when\n");
    append_list(&mut body, "", &assessment.revisit_when);
    body
}

fn append_list(body: &mut String, heading: &str, values: &[String]) {
    if values.is_empty() {
        return;
    }
    if !heading.is_empty() {
        body.push_str(&format!("\n## {heading}\n"));
    }
    for value in values {
        body.push_str(&format!("\n- {}", value.trim()));
    }
    body.push('\n');
}

fn append_labeled_list(body: &mut String, label: &str, values: &[String]) {
    if !values.is_empty() {
        body.push_str(&format!("\n**{label}**\n"));
        for value in values {
            body.push_str(&format!("\n- {}", value.trim()));
        }
        body.push('\n');
    }
}

fn append_inline(body: &mut String, label: &str, values: &[String]) {
    if !values.is_empty() {
        body.push_str(&format!("; {label}: {}", values.join("; ")));
    }
}

fn matching_decision<R: PlanningRoot + ?Sized>(
    repo: &R,
    directory: &Path,
    item_id: &str,
) -> anyhow::Result<Option<(PathBuf, String)>> {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries.collect::<Result<Vec<_>, _>>()?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let marker = format!("Decision item: `{item_id}`");
    for entry in entries {
        if !entry
            .file_type()
            .is_ok_and(|kind| kind.is_file() && !kind.is_symlink())
            || entry
                .path()
                .extension()
                .is_none_or(|extension| extension != "md")
        {
            continue;
        }
        let path = entry.path();
        let previous = String::from_utf8(repo.read_planning_path(&path)?)?;
        if previous.lines().any(|line| line == marker) {
            return Ok(Some((path, previous)));
        }
    }
    Ok(None)
}

fn stable_visible(markdown: &str) -> String {
    ArtifactIdentity::visible_markdown(markdown)
        .lines()
        .map(|line| {
            if line.starts_with("Date: ") {
                "Date: <approved>"
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn ensure_path_has_no_symlinks(repo: &Path, target: &Path) -> anyhow::Result<()> {
    let relative = target.strip_prefix(repo)?;
    anyhow::ensure!(
        relative
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_))),
        "ADR path contains an invalid component"
    );

    let mut path = PathBuf::new();
    for component in repo.components() {
        path.push(component.as_os_str());
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) => {
                anyhow::ensure!(
                    !metadata.file_type().is_symlink(),
                    "ADR path contains a symlink: {}",
                    path.display()
                );
                anyhow::ensure!(
                    metadata.is_dir(),
                    "ADR parent is not a directory: {}",
                    path.display()
                );
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        }
    }

    let components = relative.components().collect::<Vec<_>>();
    for (index, component) in components.iter().enumerate() {
        path.push(component.as_os_str());
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) => {
                anyhow::ensure!(
                    !metadata.file_type().is_symlink(),
                    "ADR path contains a symlink: {}",
                    path.display()
                );
                if index + 1 < components.len() {
                    anyhow::ensure!(metadata.is_dir(), "ADR parent is not a directory");
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn relative_path(
    store: &crate::artifacts::planning_store::PlanningStore,
    path: &Path,
) -> anyhow::Result<String> {
    let relative = path.strip_prefix(&store.root)?.to_string_lossy();
    Ok(store.git_path(&relative))
}

#[cfg(test)]
#[path = "decision_tests.rs"]
mod tests;
