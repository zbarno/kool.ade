use super::repository_evidence;

#[cfg(test)]
#[path = "prompt/tests.rs"]
mod tests;

fn capability_plan_guidance(
    target: &crate::core::opportunity::OpportunityTargetKindV1,
) -> &'static str {
    if *target == crate::core::opportunity::OpportunityTargetKindV1::KooladeProduct {
        " This is a pursued Kool.ad/e product improvement: the plan must include a `## Capability ID` section with a stable, lower-case product capability slug. This identifier is part of the user-confirmed plan and must not be a run, task, or feature UUID. The implementation plan must add that exact ID to the compiled active-capability inventory with an app-owned feature-specific readiness probe, and must not claim readiness until the probe is implemented. The current self-improvement lifecycle capability uses `self-improvement-lifecycle` with probe `self-improvement-lifecycle-v1`; choosing another ID requires implementing and registering its matching probe in the same reviewed change."
    } else {
        ""
    }
}

fn opportunity_planner_context(
    project: &super::super::session::Project,
    feature_text: &str,
    feature_uid: &str,
) -> Result<Option<crate::core::opportunity::OpportunityV1>, String> {
    let marker = "<!-- opportunity-uid: ";
    let Some(uid) = feature_text
        .lines()
        .find_map(|line| line.trim().strip_prefix(marker)?.strip_suffix(" -->"))
    else {
        return Ok(None);
    };
    let (proposal, _) =
        crate::core::opportunity::OpportunityStore::new(&project.state.planning_store)
            .load(uid)
            .map_err(|error| format!("could not load pursued opportunity context: {error}"))?;
    if proposal.opportunity_uid != uid
        || !proposal
            .related_feature_uids
            .iter()
            .any(|uid| uid == feature_uid)
        || !matches!(
            proposal.status,
            crate::core::opportunity::OpportunityStatusV1::Planning
                | crate::core::opportunity::OpportunityStatusV1::Implemented
        )
    {
        return Err("the pursued opportunity no longer matches this planning feature".into());
    }
    Ok(Some(proposal))
}

fn deferred_context(question: &crate::core::planning::QuestionV1) -> String {
    question.deferred.as_ref().map_or_else(
        || "(none)".to_owned(),
        |item| {
            format!(
                "Assumption: {}; owner: {}; risk: {}; verification test: {}; follow-up task: {}",
                item.assumption, item.owner, item.risk, item.verification_test, item.follow_up_task
            )
        },
    )
}

pub(super) fn render(
    project: &super::super::session::Project,
    feature_uid: &str,
    plan_revision: u64,
) -> Result<
    (
        crate::harness::prompt_catalog::RenderedPrompt,
        crate::harness::prompt_catalog::OutputValidationContext<'static>,
    ),
    String,
> {
    use crate::harness::prompt_catalog::{
        EvidenceConfidentiality, EvidenceItem, EvidenceOrigin, PromptRole, RoleInputV1,
        RolePayloadV1,
    };
    let (feature_id, feature_text) = project
        .state
        .active_feature
        .as_ref()
        .filter(|(_, text)| {
            crate::domain::ArtifactIdentity::from_markdown(text)
                .ok()
                .flatten()
                .is_some_and(|identity| identity.uid == feature_uid)
        })
        .map(|(feature_id, text)| (feature_id.as_str(), text.as_str()))
        .ok_or_else(|| "the clarified feature is no longer active".to_owned())?;
    let identity = crate::domain::ArtifactIdentity::from_markdown(feature_text)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "the clarified feature has no stable identity".to_owned())?;
    let opportunity_context = opportunity_planner_context(project, feature_text, feature_uid)?;
    let capability_plan_guidance = opportunity_context.as_ref().map_or("", |proposal| {
        capability_plan_guidance(&proposal.target.kind)
    });
    let contract = crate::core::workflow::feature_contract(feature_text);
    let snapshot = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
        &project.state.planning_store,
        feature_id,
        &identity,
        feature_text,
        &contract,
    )?;
    let questions = crate::core::planning::QuestionStore::new(&project.state.planning_store)
        .list()
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|question| question.feature_uid == feature_uid)
        .collect::<Vec<_>>();
    let open_questions = questions
        .iter()
        .filter(|question| question.status == crate::core::planning::QuestionStatus::Open)
        .map(|question| format!("{}: {}", question.uid, question.question))
        .collect::<Vec<_>>();
    let previous_answers = questions
        .iter()
        .filter(|question| {
            matches!(
                question.status,
                crate::core::planning::QuestionStatus::Answered
                    | crate::core::planning::QuestionStatus::EvidenceResolved
                    | crate::core::planning::QuestionStatus::Declined
            ) || (question.status == crate::core::planning::QuestionStatus::Superseded
                && (question.answered_by.is_some() || question.answer_text.is_some()))
        })
        .map(|question| {
            format!(
                "{}: {}",
                question.question,
                question
                    .answer_text
                    .as_deref()
                    .or(question.selected_option.as_deref())
                    .unwrap_or("(declined; no answer was inferred)")
            )
        })
        .collect::<Vec<_>>();
    let requirements: Vec<String> = snapshot
        .requirements
        .into_iter()
        .map(|requirement| requirement.stable_id)
        .collect();
    let workspace_uid = project
        .state
        .planning_store
        .project_id
        .hyphenated()
        .to_string();
    let feature_evidence_id = format!("feature:{feature_uid}");
    let repository_evidence_id = format!("repository-overview:{feature_uid}");
    let mut evidence_manifest = vec![
        EvidenceItem {
            id: feature_evidence_id.clone(),
            workspace_uid: workspace_uid.clone(),
            origin: EvidenceOrigin::Application,
            confidentiality: EvidenceConfidentiality::WorkspacePrivate,
            source: format!("planning feature specification {feature_uid}"),
            content: crate::core::context_build::clip_bytes(feature_text, 16 * 1024),
        },
        repository_evidence::for_feature(&project.state, feature_uid, &workspace_uid),
    ];
    let mut evidence_ids = std::collections::HashSet::from([
        feature_evidence_id.clone(),
        repository_evidence_id.clone(),
    ]);
    for question in &questions {
        let id = format!("clarification:{}", question.uid);
        evidence_ids.insert(id.clone());
        let deferred = deferred_context(question);
        evidence_manifest.push(EvidenceItem {
            id,
            workspace_uid: workspace_uid.clone(),
            origin: EvidenceOrigin::Application,
            confidentiality: EvidenceConfidentiality::WorkspacePrivate,
            source: format!("persisted clarification {}", question.uid),
            content: format!(
                "Question: {}\nReason: {}\nStatus: {:?}\nAnswer: {}\nSelected option: {}\nExplicit deferral: {}",
                question.question,
                question.reason,
                question.status,
                question.answer_text.as_deref().unwrap_or("(none)"),
                question.selected_option.as_deref().unwrap_or("(none)"),
                deferred
            ),
        });
    }
    let mut research_evidence = evidence_ids.iter().cloned().collect::<Vec<_>>();
    research_evidence.sort();
    let input = RoleInputV1 {
        role: PromptRole::Planner,
        workspace_uid: workspace_uid.clone(),
        feature_uid: feature_uid.to_owned(),
        task_uid: None,
        original_goal: crate::core::context_build::clip_bytes(&contract, 16 * 1024),
        approved_spec_ref: None,
        approved_spec_hash: None,
        decision_refs: questions
            .iter()
            .map(|question| question.uid.clone())
            .collect(),
        evidence_manifest,
        execution_policy_ref: "planner-policy/application-persistence-only/v1".into(),
        output_schema_version: PromptRole::Planner.output_schema_version().into(),
        payload: RolePayloadV1::PlannerOpportunity {
            open_questions,
            previous_answers,
            research_evidence,
            requested_scope: format!(
                "Reassess feature plan revision {plan_revision} after the recorded clarification. Inspect the supplied read-only repository overview before asking. Use strong repository precedent to resolve technical choices, cite its evidence ID and rationale, and ask only when a choice changes user intent or exceeds agent authority. Put an evidence-backed resolution for an existing technical question in resolvedQuestions. Only resolve Integration, Api, Performance, Operations, Testing, or Deployment choices; never resolve user intent, scope, UX, security, privacy, data ownership, or failure policy for the user. Do not repeat a resolved question. Preserve explicit optional deferrals with their owner, risk, verification test, and follow-up task; do not treat a deferral as a product decision. Propose only material new questions and traceable readiness evidence.{}",
                capability_plan_guidance
            ),
            opportunity_origin: opportunity_context
                .as_ref()
                .map(|proposal| format!("{:?}", proposal.origin)),
            existing_support_evidence: opportunity_context
                .as_ref()
                .map(|proposal| {
                    proposal
                        .evidence_refs
                        .iter()
                        .map(|evidence| format!("{}#{}", evidence.evidence_uid, evidence.sha256))
                        .collect()
                })
                .unwrap_or_default(),
            workaround: opportunity_context
                .as_ref()
                .and_then(|proposal| proposal.generic_workaround.clone()),
            user_value: opportunity_context
                .as_ref()
                .map(|proposal| proposal.user_value.clone()),
            confidentiality_class: if opportunity_context.as_ref().is_some_and(|proposal| {
                proposal.evidence_refs.iter().any(|evidence| {
                    evidence.confidentiality
                        == crate::core::opportunity::EvidenceConfidentialityV1::UserLocal
                })
            }) {
                EvidenceConfidentiality::Restricted
            } else {
                EvidenceConfidentiality::WorkspacePrivate
            },
            concrete_target_repo: opportunity_context.as_ref().map(|proposal| {
                format!(
                    "{:?}; repository_uid={}; source_sha={}",
                    proposal.target.kind,
                    proposal.target.repository_uid,
                    proposal.target.source_sha
                )
            }),
        },
    };
    let rendered = crate::harness::prompt_catalog::render(&input)?;
    Ok((
        rendered,
        crate::harness::prompt_catalog::OutputValidationContext {
            evidence_ids,
            allowed_question_uids: questions
                .iter()
                .filter(|question| {
                    question.status == crate::core::planning::QuestionStatus::Open
                        && question.plan_revision == plan_revision
                })
                .map(|question| question.uid.clone())
                .collect(),
            required_requirements: requirements.into_iter().collect(),
            ..Default::default()
        },
    ))
}
