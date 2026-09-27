use super::*;

pub(super) fn resolve_feature(app: &PacketApp, target: Option<&str>) -> Result<String, String> {
    let Screen::Connected(project) = &app.screen else {
        return Err("Connect a project before approving a change.".into());
    };
    let candidates = project
        .state
        .active_features
        .iter()
        .filter(|(_, body)| {
            crate::domain::ChangeMetadata::require_markdown(body)
                .is_ok_and(|metadata| metadata.status.approval_eligible())
        })
        .filter(|(id, body)| {
            target.is_none_or(|target| {
                *id == target
                    || crate::domain::ArtifactIdentity::from_markdown(body)
                        .ok()
                        .flatten()
                        .is_some_and(|identity| {
                            identity.uid == target || identity.display_id == target
                        })
            })
        })
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();
    if let Some(target) = target {
        return candidates.into_iter().next().ok_or_else(|| {
            format!("No current ready change matches ID {target}. Review the active feature list.")
        });
    }
    if let Some((active, _)) = &project.state.active_feature
        && candidates.iter().any(|id| id == active)
    {
        return Ok(active.clone());
    }
    match candidates.as_slice() {
        [only] => Ok(only.clone()),
        [] => Err("No change is currently ready for approval.".into()),
        _ => Err("Several changes are ready. Specify which change to approve.".into()),
    }
}

pub(super) fn task_matches(
    document: &crate::artifacts::task_docs::TaskDocument,
    target: &str,
) -> bool {
    document
        .identity
        .as_ref()
        .is_some_and(|identity| identity.uid == target || identity.display_id == target)
}

pub(super) fn task_title(
    documents: &[crate::artifacts::task_docs::TaskDocument],
    ticket: &str,
) -> String {
    documents
        .iter()
        .find(|document| document.path == ticket)
        .map(|document| document.title.clone())
        .unwrap_or_else(|| "the task".into())
}
