use crate::core::planning::QuestionV1;

pub(super) type PreparedDocumentsAndRevision = (Vec<(String, Vec<u8>)>, String);

pub(super) fn prepare(
    store: &crate::artifacts::planning_store::PlanningStore,
    feature_uid: &str,
    expected_spec_revision: &str,
    resolved: &[(QuestionV1, u64)],
) -> Result<PreparedDocumentsAndRevision, String> {
    if resolved.is_empty() {
        return Ok((Vec::new(), expected_spec_revision.to_owned()));
    }

    let (feature_id, mut markdown) = crate::artifacts::product_docs::active_features(store)
        .into_iter()
        .find(|(_, markdown)| {
            crate::domain::ArtifactIdentity::from_markdown(markdown)
                .ok()
                .flatten()
                .is_some_and(|identity| identity.uid == feature_uid)
        })
        .ok_or_else(|| "resolved clarification feature is no longer active".to_owned())?;
    let identity = crate::domain::ArtifactIdentity::from_markdown(&markdown)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "resolved clarification feature has no stable identity".to_owned())?;
    let original = snapshot(store, &feature_id, &identity, &markdown)?;
    if original.spec_revision != expected_spec_revision {
        return Err("technical resolution belongs to a stale feature snapshot".into());
    }

    for (question, _) in resolved {
        if question.feature_uid != feature_uid {
            return Err("technical resolution does not belong to the active feature".into());
        }
        let (decision, rationale) = parse_resolution(question)?;
        let evidence = question
            .evidence_refs
            .iter()
            .map(|reference| safe_text(reference))
            .collect::<Vec<_>>()
            .join(", ");
        let marker = format!("<!-- koolade-clarification-resolution:{} -->", question.uid);
        let record = format!(
            "{marker}\n- **Repository-evidenced technical decision {}:** {}\n  - Rationale: {}\n  - Evidence: {}\n",
            question.uid,
            safe_text(decision),
            safe_text(rationale),
            evidence
        );
        append_or_verify(&mut markdown, &record)?;
    }

    let updated = snapshot(store, &feature_id, &identity, &markdown)?;
    let path =
        crate::artifacts::product_docs::document_path(store, &format!("feature:{feature_id}"))
            .map_err(|error| error.to_string())?;
    let relative = path
        .strip_prefix(&store.root)
        .map_err(|_| "feature specification is outside the planning store".to_owned())?
        .to_string_lossy()
        .into_owned();
    Ok((
        vec![(relative, markdown.into_bytes())],
        updated.spec_revision,
    ))
}

fn snapshot(
    store: &crate::artifacts::planning_store::PlanningStore,
    feature_id: &str,
    identity: &crate::domain::ArtifactIdentity,
    markdown: &str,
) -> Result<crate::core::lifecycle::PlanSnapshotV1, String> {
    let contract = crate::core::workflow::feature_contract(markdown);
    crate::core::lifecycle::plan_snapshot::from_feature_with_author(
        store, feature_id, identity, markdown, &contract,
    )
}

fn parse_resolution(question: &QuestionV1) -> Result<(&str, &str), String> {
    if let Some(resolution) = &question.evidence_resolution {
        return Ok((&resolution.decision, &resolution.rationale));
    }
    // Older persisted records only have the human-readable projection.
    let answer = question
        .answer_text
        .as_deref()
        .ok_or_else(|| "evidence-resolved question has no recorded decision".to_owned())?;
    let (decision, rationale) = answer
        .strip_prefix("Decision: ")
        .and_then(|answer| answer.split_once("\nRationale: "))
        .ok_or_else(|| "evidence-resolved question has a malformed decision record".to_owned())?;
    Ok((decision, rationale))
}

fn append_or_verify(markdown: &mut String, record: &str) -> Result<(), String> {
    let marker = record
        .lines()
        .next()
        .ok_or_else(|| "technical decision record is empty".to_owned())?;
    let mut occurrences = markdown.match_indices(marker);
    if let Some((index, _)) = occurrences.next() {
        if occurrences.next().is_some() || !markdown[index..].starts_with(record) {
            return Err("existing technical decision marker conflicts with its record".into());
        }
        return Ok(());
    }
    append_to_decisions(markdown, record);
    Ok(())
}

fn append_to_decisions(markdown: &mut String, record: &str) {
    const HEADING: &str = "## Decisions and Assumptions";
    if let Some(heading) = markdown.find(HEADING) {
        let content_start = heading + HEADING.len();
        let content_end = markdown[content_start..]
            .find("\n## ")
            .map_or(markdown.len(), |offset| content_start + offset);
        markdown.insert_str(content_end, &format!("\n{record}"));
    } else {
        markdown.push_str(&format!("\n\n{HEADING}\n\n{record}"));
    }
}

fn safe_text(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('`', "&#96;")
        .replace('*', "&#42;")
        .replace('_', "&#95;")
        .replace('[', "&#91;")
        .replace(']', "&#93;")
}

#[cfg(test)]
#[path = "technical_decisions/tests.rs"]
mod tests;
