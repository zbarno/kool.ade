use super::feature_index::directory_feature_id;
use std::path::Path;

pub fn active_feature(repo: &Path) -> Option<(String, String)> {
    active_features(repo).into_iter().next()
}

/// Select the current planning focus by the durable brief title, falling back
/// to the first active feature for legacy projects without a matching brief.
pub fn active_feature_for_workflow(
    repo: &Path,
    workflow: &crate::core::workflow::Workflow,
) -> Option<(String, String)> {
    select_active_feature(&active_features(repo), workflow)
}

fn select_active_feature(
    features: &[(String, String)],
    workflow: &crate::core::workflow::Workflow,
) -> Option<(String, String)> {
    let matches = workflow.brief.as_ref().map(|brief| {
        let wanted = normalize_title(&brief.feature_name);
        features
            .iter()
            .filter(|(id, body)| {
                body.lines()
                    .find_map(|line| line.strip_prefix("# "))
                    .and_then(|heading| heading.split_once(": ").map(|(_, title)| title))
                    .is_some_and(|title| {
                        let title = normalize_title(title);
                        title == wanted
                            || wanted
                                .strip_suffix(&normalize_title(id))
                                .is_some_and(|without_id| without_id == title)
                    })
            })
            .collect::<Vec<_>>()
    });
    matches
        .filter(|matches| matches.len() == 1)
        .and_then(|matches| matches.first().map(|(id, body)| (id.clone(), body.clone())))
        .or_else(|| features.first().cloned())
}

fn normalize_title(title: &str) -> String {
    title
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Every feature that is still active. Feature status is independent, so
/// several deltas may be planned or implemented at the same time.
pub fn active_features(repo: &Path) -> Vec<(String, String)> {
    let layout = crate::artifacts::layout::ArtifactLayout::new(repo);
    active_feature_directories(repo)
        .into_iter()
        .filter_map(|name| {
            let id = directory_feature_id(&name)?.to_string();
            let body = std::fs::read_to_string(layout.change_specification(&name)?).ok()?;
            let metadata = crate::domain::ChangeMetadata::require_markdown(&body).ok()?;
            let body = crate::domain::ChangeMetadata::render_status(&body, metadata.status).ok()?;
            Some((id, body))
        })
        .collect()
}

pub(super) fn active_feature_directories(repo: &Path) -> Vec<String> {
    let layout = crate::artifacts::layout::ArtifactLayout::new(repo);
    let Ok(entries) = std::fs::read_dir(layout.changes_root()) else {
        return Vec::new();
    };
    let mut entries = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let path = layout.change_specification(&name)?;
            let body = std::fs::read_to_string(path).ok()?;
            let status = crate::domain::ChangeMetadata::require_markdown(&body)
                .ok()?
                .status;
            if status.is_terminal() {
                return None;
            }
            Some(name)
        })
        .collect::<Vec<_>>();
    entries.sort();
    entries
}

pub fn validate_change_metadata(repo: &Path) -> anyhow::Result<()> {
    let layout = crate::artifacts::layout::ArtifactLayout::new(repo);
    for name in active_feature_directories_all(repo)? {
        let path = layout
            .change_specification(&name)
            .ok_or_else(|| anyhow::anyhow!("Invalid change directory {name}"))?;
        let markdown = std::fs::read_to_string(&path)?;
        let identity = crate::domain::ArtifactIdentity::from_markdown(&markdown)?
            .ok_or_else(|| anyhow::anyhow!("Change {} has no stable identity", path.display()))?;
        let metadata = crate::domain::ChangeMetadata::from_markdown(&markdown)?
            .ok_or_else(|| anyhow::anyhow!("Change {} has no structured status", path.display()))?;
        anyhow::ensure!(
            metadata.uid == identity.uid && metadata.display_id == identity.display_id,
            "Change status identity does not match {}",
            path.display()
        );
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn migrate_legacy_change_fixtures(repo: &Path) -> anyhow::Result<()> {
    let layout = crate::artifacts::layout::ArtifactLayout::new(repo);
    for name in active_feature_directories_all(repo)? {
        let Some(path) = layout.change_specification(&name) else {
            continue;
        };
        let markdown = std::fs::read_to_string(&path)?;
        if crate::domain::ChangeMetadata::from_markdown(&markdown)?.is_some() {
            continue;
        }
        let heading = markdown
            .lines()
            .find_map(|line| line.strip_prefix("# "))
            .ok_or_else(|| anyhow::anyhow!("Fixture change {} has no heading", path.display()))?;
        let (display_id, title) = heading.split_once(": ").ok_or_else(|| {
            anyhow::anyhow!("Fixture change {} has no display ID", path.display())
        })?;
        let previous = crate::domain::ArtifactIdentity::from_markdown(&markdown)?;
        let identified = crate::domain::ArtifactIdentity::preserve_markdown(
            &markdown,
            previous.as_ref().map(|_| markdown.as_str()),
            display_id,
            title,
        )?;
        let identity = crate::domain::ArtifactIdentity::from_markdown(&identified)?
            .ok_or_else(|| anyhow::anyhow!("Fixture identity was not written"))?;
        let status = crate::domain::ChangeMetadata::parse_legacy_markdown(&identified)?;
        let migrated =
            crate::domain::ChangeMetadata::write_markdown(&identified, &identity, status)?;
        std::fs::write(path, migrated)?;
    }
    Ok(())
}

fn active_feature_directories_all(repo: &Path) -> anyhow::Result<Vec<String>> {
    let root = crate::artifacts::layout::ArtifactLayout::new(repo).changes_root();
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| anyhow::anyhow!("Change directory name is not UTF-8"))?;
        if crate::artifacts::layout::ArtifactLayout::new(repo)
            .change_specification(&name)
            .is_some_and(|path| path.is_file())
        {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_brief_selects_its_feature_instead_of_lexical_first() {
        let features = vec![
            ("CHG-003".into(), "# CHG-003: Readable Replies\n".into()),
            ("F7".into(), "# F7: Operator-Chosen Display Names\n".into()),
        ];
        let workflow = crate::core::workflow::Workflow {
            brief: Some(crate::core::workflow::InterviewBrief {
                feature_name: "Operator-Chosen Display Names".into(),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(select_active_feature(&features, &workflow).unwrap().0, "F7");
    }

    #[test]
    fn workflow_brief_with_appended_feature_id_selects_its_feature() {
        let features = vec![
            ("CHG-003".into(), "# CHG-003: Readable Replies\n".into()),
            ("F7".into(), "# F7: Comparative Plan Approval\n".into()),
        ];
        let workflow = crate::core::workflow::Workflow {
            brief: Some(crate::core::workflow::InterviewBrief {
                feature_name: "Comparative Plan Approval (F7)".into(),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(select_active_feature(&features, &workflow).unwrap().0, "F7");
    }

    #[test]
    fn unmatched_or_ambiguous_brief_keeps_the_legacy_fallback() {
        let features = vec![
            ("CHG-003".into(), "# CHG-003: Duplicate\n".into()),
            ("F7".into(), "# F7: Duplicate\n".into()),
        ];
        let workflow = crate::core::workflow::Workflow {
            brief: Some(crate::core::workflow::InterviewBrief {
                feature_name: "Duplicate".into(),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(
            select_active_feature(&features, &workflow).unwrap().0,
            "CHG-003"
        );
    }
}
