use super::feature_index::directory_feature_id;
use crate::artifacts::planning_store::PlanningRoot;

pub fn active_feature<R: PlanningRoot + ?Sized>(repo: &R) -> Option<(String, String)> {
    active_features(repo).into_iter().next()
}

/// Select the current planning focus by the durable brief title.
pub fn active_feature_for_workflow<R: PlanningRoot + ?Sized>(
    repo: &R,
    workflow: &crate::core::workflow::Workflow,
) -> Option<(String, String)> {
    active_feature_for_workflow_in(&active_features(repo), workflow)
}

/// Read live feature documents from the primary code repository at its
/// immutable published-spec pin. Local task state is never their authority.
pub fn published_features(
    reader: &crate::core::published_specs::PublishedSpecReader,
    pin: &crate::core::published_specs::PublishedSpecPin,
) -> anyhow::Result<Vec<(String, String)>> {
    let mut features = std::collections::BTreeMap::new();
    for document in reader
        .read_documentation(pin)?
        .into_iter()
        .filter(|document| {
            document.path.starts_with("docs/koolade/features/")
                && document.path.ends_with(".md")
                && !document.path.ends_with("/AGENTS.md")
        })
    {
        let Some(identity) = crate::domain::ArtifactIdentity::from_markdown(&document.content)?
        else {
            // Documentation directories may contain ordinary project docs
            // alongside managed feature records. Only stable-ID records are
            // feature specifications.
            continue;
        };
        let filename = std::path::Path::new(&document.path)
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| anyhow::anyhow!("published feature path is invalid"))?;
        anyhow::ensure!(
            filename == identity.uid,
            "published feature path does not match its immutable identity"
        );
        crate::core::specification::validate_feature(&identity.display_id, &document.content)?;
        let metadata = crate::domain::ChangeMetadata::require_markdown(&document.content)?;
        anyhow::ensure!(
            metadata.uid == identity.uid && metadata.display_id == identity.display_id,
            "published feature metadata does not match its identity"
        );
        if !metadata.status.is_terminal() {
            let body =
                crate::domain::ChangeMetadata::render_status(&document.content, metadata.status)?;
            anyhow::ensure!(
                features.insert(identity.display_id.clone(), body).is_none(),
                "published feature display ID is duplicated"
            );
        }
    }
    Ok(features.into_iter().collect())
}

pub fn active_feature_for_workflow_in(
    features: &[(String, String)],
    workflow: &crate::core::workflow::Workflow,
) -> Option<(String, String)> {
    select_active_feature(features, workflow)
}

fn select_active_feature(
    features: &[(String, String)],
    workflow: &crate::core::workflow::Workflow,
) -> Option<(String, String)> {
    let Some(brief) = workflow.brief.as_ref() else {
        return features.first().cloned();
    };
    let wanted = normalize_title(&brief.feature_name);
    let matches = features
        .iter()
        .filter(|(id, body)| {
            body.lines()
                .find_map(|line| line.strip_prefix("# "))
                .and_then(|heading| heading.split_once(": ").map(|(_, title)| title))
                .is_some_and(|title| {
                    let title = normalize_title(title);
                    let feature_id = normalize_title(id);
                    title == wanted
                        || wanted
                            .strip_prefix(&feature_id)
                            .is_some_and(|without_id| without_id == title)
                        || wanted
                            .strip_suffix(&feature_id)
                            .is_some_and(|without_id| without_id == title)
                })
        })
        .collect::<Vec<_>>();
    (matches.len() == 1).then(|| {
        let (id, body) = matches[0];
        (id.clone(), body.clone())
    })
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
pub fn active_features<R: PlanningRoot + ?Sized>(repo: &R) -> Vec<(String, String)> {
    let store = repo.planning_store();
    if let Some(code_root) = store.published_repository_root.as_deref() {
        let loaded = (|| -> anyhow::Result<_> {
            let reader = crate::core::published_specs::PublishedSpecReader::for_primary_repository(
                code_root,
            )?;
            let pin = match &store.published_spec_pin {
                Some(pin) => pin.clone(),
                None => reader.pin()?,
            };
            let mut features = published_features(&reader, &pin)?;
            let draft = crate::core::apply::load_current(&store)?;
            crate::core::state::proposals::apply_local_feature_proposals(
                &mut features,
                draft.as_ref(),
                Some(&pin),
            )?;
            Ok(features)
        })();
        return loaded.unwrap_or_default();
    }
    let layout = repo.planning_layout();
    active_feature_directories(repo)
        .into_iter()
        .filter_map(|name| {
            let id = directory_feature_id(&name)?.to_string();
            let path = layout.change_specification(&name)?;
            let body = String::from_utf8(repo.read_planning_path(&path).ok()?).ok()?;
            let metadata = crate::domain::ChangeMetadata::require_markdown(&body).ok()?;
            let body = crate::domain::ChangeMetadata::render_status(&body, metadata.status).ok()?;
            Some((id, body))
        })
        .collect()
}

pub(super) fn active_feature_directories<R: PlanningRoot + ?Sized>(repo: &R) -> Vec<String> {
    let layout = repo.planning_layout();
    let Ok(entries) = std::fs::read_dir(layout.changes_root()) else {
        return Vec::new();
    };
    let mut entries = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let path = layout.change_specification(&name)?;
            let body = String::from_utf8(repo.read_planning_path(&path).ok()?).ok()?;
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

pub fn validate_change_metadata<R: PlanningRoot + ?Sized>(repo: &R) -> anyhow::Result<()> {
    let layout = repo.planning_layout();
    for name in active_feature_directories_all(repo)? {
        let path = layout
            .change_specification(&name)
            .ok_or_else(|| anyhow::anyhow!("Invalid change directory {name}"))?;
        let markdown = String::from_utf8(repo.read_planning_path(&path)?)?;
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

fn active_feature_directories_all<R: PlanningRoot + ?Sized>(
    repo: &R,
) -> anyhow::Result<Vec<String>> {
    let root = repo.planning_layout().changes_root();
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
        if repo
            .planning_layout()
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
    fn workflow_brief_with_prefixed_feature_id_selects_its_feature() {
        let features = vec![
            ("CHG-003".into(), "# CHG-003: Readable Replies\n".into()),
            ("F7".into(), "# F7: Comparative Plan Approval\n".into()),
        ];
        let workflow = crate::core::workflow::Workflow {
            brief: Some(crate::core::workflow::InterviewBrief {
                feature_name: "F7: Comparative Plan Approval".into(),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(select_active_feature(&features, &workflow).unwrap().0, "F7");
    }

    #[test]
    fn unmatched_or_ambiguous_brief_does_not_select_an_unrelated_feature() {
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
        assert_eq!(select_active_feature(&features, &workflow), None);
    }
}
