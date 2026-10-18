use crate::artifacts::planning_store::PlanningRoot;

pub(super) fn create_draft_feature(
    store: &crate::artifacts::planning_store::PlanningStore,
    proposal: &crate::core::opportunity::OpportunityV1,
) -> anyhow::Result<(String, String, String, String)> {
    use crate::domain::ChangeStatus;
    if let Some(existing) = existing_opportunity_feature(store, &proposal.opportunity_uid)? {
        return Ok(existing);
    }
    let feature_id = crate::artifacts::product_docs::next_feature_id(store);
    let title = feature_title(&proposal.problem_or_opportunity);
    let body = format!(
        "# {feature_id}: {title}\n\n<!-- opportunity-uid: {} -->\n\n## Intent\n\n{}\n\n## Current Behavior\n\n{}\n\n## Desired Behavior\n\n{}\n\n## Scope\n\nProposed approach: {}\n\nAlternatives considered: {}\n\n## Affected Product Areas\n\n{}\n\n## Requirements\n\n- {}\n\n## Decisions and Assumptions\n\n- Origin: {:?}.\n- Target repository kind: {:?}.\n- Target repository ID: `{}`.\n- Observed source revision: `{}`.\n- Evidence references: {}.\n- The local controller recorded an explicit Pursue choice for opportunity `{}`.\n- The proposal remains advisory; this draft must be reviewed and approved through the normal feature workflow.\n\n## Acceptance Criteria\n\n- {}\n",
        proposal.opportunity_uid,
        proposal.user_value,
        proposal.current_capability_state,
        proposal.expected_outcome,
        proposal.proposed_approach,
        proposal.alternatives.join("; "),
        proposal
            .affected_systems
            .iter()
            .map(|area| format!("- {area}"))
            .collect::<Vec<_>>()
            .join("\n"),
        proposal.expected_outcome,
        proposal.origin,
        proposal.target.kind,
        proposal.target.repository_uid,
        proposal.target.source_sha,
        proposal
            .evidence_refs
            .iter()
            .map(|evidence| format!(
                "{}#{} ({:?})",
                evidence.evidence_uid, evidence.sha256, evidence.confidentiality
            ))
            .collect::<Vec<_>>()
            .join(", "),
        proposal.opportunity_uid,
        proposal.expected_outcome,
    );
    crate::core::specification::validate_feature(&feature_id, &body)?;
    let identified =
        crate::artifacts::product_docs::identity::preserve_feature_identity_with_previous(
            None,
            &feature_id,
            &body,
        )?;
    let identity = crate::domain::ArtifactIdentity::from_markdown(&identified)?
        .ok_or_else(|| anyhow::anyhow!("feature identity was not created"))?;
    let markdown =
        crate::domain::ChangeMetadata::write_markdown(&identified, &identity, ChangeStatus::Draft)?;
    let logical_id = format!("feature:{feature_id}");
    let path =
        crate::artifacts::product_docs::document_path_for_update(store, &logical_id, &markdown)?;
    let layout = store.planning_layout();
    let relative = path
        .strip_prefix(layout.root())?
        .to_string_lossy()
        .into_owned();
    let index =
        crate::artifacts::product_docs::refreshed_index(store, &[(logical_id, markdown.clone())])?;
    let current = store.revision()?;
    let (_, revision) = store.transaction_with_revision(
        &[
            (relative, markdown.as_bytes().to_vec()),
            (
                crate::artifacts::planning_store::paths::PRODUCT_INDEX.into(),
                index.into_bytes(),
            ),
        ],
        Some(&current),
    )?;
    Ok((feature_id, identity.uid, markdown, revision))
}

fn existing_opportunity_feature(
    store: &crate::artifacts::planning_store::PlanningStore,
    uid: &str,
) -> anyhow::Result<Option<(String, String, String, String)>> {
    use crate::artifacts::planning_store::PlanningRoot;
    let root = store.planning_layout().changes_root();
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let marker = format!("<!-- opportunity-uid: {uid} -->");
    for entry in entries {
        let entry = entry?;
        let metadata = std::fs::symlink_metadata(entry.path())?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            continue;
        }
        let specification = entry.path().join("specification.md");
        let metadata = match std::fs::symlink_metadata(&specification) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            continue;
        }
        let markdown = std::fs::read_to_string(specification)?;
        if !markdown.contains(&marker) {
            continue;
        }
        let feature_id = markdown
            .lines()
            .next()
            .and_then(|line| line.strip_prefix("# "))
            .and_then(|line| line.split_once(": ").map(|(id, _)| id))
            .ok_or_else(|| anyhow::anyhow!("adopted opportunity feature has no valid title"))?;
        let identity = crate::domain::ArtifactIdentity::from_markdown(&markdown)?
            .ok_or_else(|| anyhow::anyhow!("adopted opportunity feature has no identity"))?;
        return Ok(Some((
            feature_id.to_owned(),
            identity.uid,
            markdown,
            store.revision()?,
        )));
    }
    Ok(None)
}

fn feature_title(problem: &str) -> String {
    let title = problem
        .split(['.', '?', '!', '\n'])
        .next()
        .unwrap_or(problem)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    crate::core::context_build::clip(&title, 72)
}
