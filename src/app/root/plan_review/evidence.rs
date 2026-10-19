use crate::{
    app::session::Project,
    core::{lifecycle::PlanSnapshotV1, planning::QuestionStatus},
    harness::prompt_catalog::{EvidenceConfidentiality, EvidenceItem, EvidenceOrigin},
};

pub(crate) struct ReviewEvidence {
    pub decision_refs: Vec<String>,
    pub items: Vec<EvidenceItem>,
}

pub(crate) fn collect(
    project: &Project,
    snapshot: &PlanSnapshotV1,
) -> Result<ReviewEvidence, String> {
    let workspace_uid = project
        .state
        .planning_store
        .project_id
        .hyphenated()
        .to_string();
    let mut items = vec![EvidenceItem {
        id: format!("plan:{}", snapshot.uid),
        workspace_uid: workspace_uid.clone(),
        origin: EvidenceOrigin::Application,
        confidentiality: EvidenceConfidentiality::WorkspacePrivate,
        source: format!("immutable approved plan snapshot {}", snapshot.uid),
        content: serde_json::to_string(snapshot).map_err(|error| error.to_string())?,
    }];
    let questions = crate::core::planning::QuestionStore::new(&project.state.planning_store)
        .list()
        .map_err(|error| format!("current planning decisions could not be loaded: {error}"))?
        .into_iter()
        .filter(|question| {
            question.feature_uid == snapshot.feature_uid
                && matches!(
                    question.status,
                    QuestionStatus::Answered
                        | QuestionStatus::EvidenceResolved
                        | QuestionStatus::Deferred
                )
        })
        .collect::<Vec<_>>();
    let decision_refs = questions
        .iter()
        .map(|question| format!("decision:{}", question.uid))
        .collect::<Vec<_>>();
    if !questions.is_empty() {
        items.push(EvidenceItem {
            id: format!("feature-decisions:{}", snapshot.feature_uid),
            workspace_uid: workspace_uid.clone(),
            origin: EvidenceOrigin::Application,
            confidentiality: EvidenceConfidentiality::WorkspacePrivate,
            source: "current answered, evidence-resolved, and deferred feature questions".into(),
            content: serde_json::to_string(&questions).map_err(|error| error.to_string())?,
        });
    }
    let (readiness, _) = crate::core::planning::QuestionStore::new(&project.state.planning_store)
        .load_readiness(&snapshot.feature_uid)
        .map_err(|error| format!("current readiness evidence could not be loaded: {error}"))?
        .ok_or_else(|| "current readiness evidence is missing".to_owned())?;
    if readiness.spec_revision.as_deref() != Some(snapshot.spec_revision.as_str())
        || !readiness.ready
    {
        return Err("readiness evidence is stale or not ready for this plan snapshot".into());
    }
    let readiness_id = format!("readiness:{}", snapshot.feature_uid);
    items.push(EvidenceItem {
        id: readiness_id.clone(),
        workspace_uid: workspace_uid.clone(),
        origin: EvidenceOrigin::Application,
        confidentiality: EvidenceConfidentiality::WorkspacePrivate,
        source: format!("current readiness assessment for {}", snapshot.feature_id),
        content: serde_json::to_string(&readiness).map_err(|error| error.to_string())?,
    });
    let mut decision_refs = decision_refs;
    decision_refs.push(readiness_id);
    let relevant_modules =
        crate::artifacts::product_docs::load_documents(&project.state.planning_store)
            .map_err(|error| format!("product specifications could not be loaded: {error}"))?
            .unwrap_or_default()
            .into_iter()
            .filter(|document| {
                snapshot
                    .architecture
                    .contains(&format!("product:{}", document.module.id))
                    || matches!(
                        document.module.id.as_str(),
                        "current-capabilities"
                            | "architecture-and-constraints"
                            | "quality-and-acceptance"
                    )
            })
            .collect::<Vec<_>>();
    for document in relevant_modules {
        let id = format!("product-spec:{}", document.module.id);
        items.push(EvidenceItem {
            id,
            workspace_uid: workspace_uid.clone(),
            origin: EvidenceOrigin::Application,
            confidentiality: EvidenceConfidentiality::WorkspacePrivate,
            source: format!("current product specification: {}", document.module.title),
            content: document.content,
        });
    }
    Ok(ReviewEvidence {
        decision_refs,
        items,
    })
}
