use super::*;

/// Called after ordinary envelope validation but before any writes.
pub fn prepare(
    state: &PlannerState,
    env: &TurnEnvelope,
    nt: &mut NormalizedTurn,
    purpose: TurnPurpose,
) -> Result<(), Vec<String>> {
    let mut workflow = state.workflow.clone();
    if purpose == TurnPurpose::GenerateTasks {
        if !workflow.ready(state.planning_contract()) {
            return Err(vec!["Task generation requires approval of a ready, current specification. Continue the interview first.".into()]);
        }
        if nt
            .spec_markdown
            .as_deref()
            .is_some_and(|s| Some(s) != state.planning_contract())
            || !nt.document_updates.is_empty()
            || !nt.added.is_empty()
            || !nt.updates.is_empty()
            || !nt.resolved.is_empty()
            || env.interview.is_some()
        {
            return Err(vec!["Task generation must use the approved specification without changing the interview or open items.".into()]);
        }
        let brief = workflow.brief.as_ref().unwrap().clone();
        validate_brief(&brief)?;
        if state
            .items
            .iter()
            .any(|i| i.priority == crate::domain::Priority::Blocking)
        {
            return Err(vec![
                "Resolve blocking questions before task generation.".into(),
            ]);
        }
        let stories = env.task_stories.clone().unwrap_or_default();
        validate_stories(&brief, &stories)?;
        let manifest = crate::core::project_repos::ProjectManifest::load(&state.repo_root)
            .map_err(|error| vec![error.to_string()])?;
        for story in &stories {
            if manifest.repositories.len() > 1 && story.target_repository.is_empty() {
                return Err(vec![
                    "Every multi-repository task needs an explicit target_repository".into(),
                ]);
            }
            let id = if story.target_repository.is_empty() {
                "root"
            } else {
                story.target_repository.as_str()
            };
            if !manifest.repositories.iter().any(|repo| repo.id == id) {
                return Err(vec![format!("Unknown target repository {id}")]);
            }
        }
        let feature_id = state.active_feature.as_ref().map(|(id, _)| id.clone());
        if let Some(id) = &feature_id
            && !feature_approved(&state.repo_root, &workflow, id)
        {
            return Err(vec![format!("{id} needs explicit implementation approval")]);
        }
        // Identity guard: a batch may only be generated for the feature the
        // interview is actually about. Drift between the brief's subject and
        // the active feature once slipped a batch stamped with one identity
        // while narrating another, deadlocking the implementation queue with
        // no visible reason.
        let declared = feature_ids_in(&brief.feature_name);
        if let Some(problem) = brief_target_problem(&declared, feature_id.as_deref(), &|id| {
            crate::artifacts::product_docs::document_path(
                &state.repo_root,
                &format!("feature:{id}"),
            )
            .is_ok()
        }) {
            return Err(vec![problem]);
        }
        nt.task_batch = Some(TaskBatch {
            brief,
            specification: state.planning_contract().unwrap().to_string(),
            feature_id,
            contract: crate::core::contract_snapshot::freeze(state)
                .map_err(|error| vec![error.to_string()])?,
            stories,
        });
        workflow.brief.as_mut().unwrap().ready_for_tasks = false;
        nt.next_question_id = None;
    } else if nt.requested_action.is_none() {
        if env
            .task_stories
            .as_ref()
            .is_some_and(|tasks| !tasks.is_empty())
            || env
                .task_outline
                .as_ref()
                .is_some_and(|tasks| !tasks.is_empty())
        {
            return Err(vec!["Task stories were returned before the user approved task generation; no changes were saved.".into()]);
        }
        // Any further interview invalidates the previous readiness decision.
        if let Some(brief) = &mut workflow.brief {
            brief.ready_for_tasks = false;
        }
        if let Some(brief) = &env.interview {
            if brief.ready_for_tasks {
                validate_brief(brief)?;
                let blocking = state.items.iter().any(|item| {
                    item.priority == crate::domain::Priority::Blocking
                        && !nt.resolved.contains(&item.id)
                        && !nt.updates.iter().any(|(id, patch)| {
                            id == &item.id
                                && patch
                                    .priority
                                    .is_some_and(|p| p != crate::domain::Priority::Blocking)
                        })
                }) || nt
                    .added
                    .iter()
                    .any(|i| i.priority == crate::domain::Priority::Blocking)
                    || nt
                        .updates
                        .iter()
                        .any(|(_, p)| p.priority == Some(crate::domain::Priority::Blocking));
                if blocking {
                    return Err(vec![
                        "Resolve blocking questions before offering task generation.".into(),
                    ]);
                }
                let updated_feature = state.active_feature.as_ref().and_then(|(id, _)| {
                    nt.document_updates
                        .iter()
                        .find(|(document, _)| document == &format!("feature:{id}"))
                        .map(|(_, body)| body.as_str())
                });
                let updated_product = if nt
                    .document_updates
                    .iter()
                    .any(|(document, _)| document.starts_with("product:"))
                {
                    crate::artifacts::product_docs::render_product_with_updates(
                        &state.repo_root,
                        &nt.document_updates,
                    )
                    .map_err(|error| vec![error.to_string()])?
                } else {
                    None
                };
                let spec = updated_feature
                    .or(nt.spec_markdown.as_deref())
                    .or(updated_product.as_deref())
                    .or(state.planning_contract())
                    .unwrap_or_default();
                if spec.trim().is_empty() {
                    return Err(vec![
                        "A specification is required before offering task generation.".into(),
                    ]);
                }
                workflow.reviewed_specification = Some(spec.to_owned());
                nt.next_question_id = None;
                if purpose == TurnPurpose::ReviewForGeneration {
                    nt.assistant_message.push_str("\n\nThe task plan is ready. The application will now check the approved contract and continue task generation.");
                } else {
                    nt.assistant_message.push_str(&format!("\n\nThe goal and scope for {} are ready to break down. Use the Generate task stories action when you want Kool.ad/e to prepare the work, or keep refining the plan.", brief.feature_name));
                }
            }
            workflow.brief = Some(brief.clone());
        }
    }
    if workflow != state.workflow {
        nt.workflow = Some(workflow);
    }
    Ok(())
}

pub(super) fn substantive(text: &str) -> bool {
    let s = text.trim();
    !s.is_empty()
        && !matches!(
            s.to_ascii_lowercase().as_str(),
            "tbd" | "todo" | "..." | "n/a"
        )
}

fn validate_brief(b: &InterviewBrief) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    for (name, value) in [
        ("feature name", &b.feature_name),
        ("problem", &b.problem),
        ("goal", &b.goal),
        ("target users", &b.target_users),
        ("intended outcome", &b.intended_outcome),
    ] {
        if !substantive(value) {
            errors.push(format!("Interview is missing {name}."));
        }
    }
    for (name, values) in [
        ("success criteria", &b.success_criteria),
        ("scope", &b.in_scope),
        ("exclusions", &b.out_of_scope),
        ("constraints", &b.constraints),
    ] {
        if values.is_empty() || values.iter().any(|v| !substantive(v)) {
            errors.push(format!(
                "Interview must establish {name} (explicitly state when none apply)."
            ));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}
