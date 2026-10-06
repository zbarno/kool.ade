use super::{
    prompt::{STORY_CONTRACT, TASK_OUTLINE_STEP},
    response::{decode_outline, specification_h1_feature_id, story_response},
    run::{Checkpoint, OUTLINE_ATTEMPTS, Run, STORY_ATTEMPTS},
};
use crate::{
    core::{state::PlannerState, workflow::validate_outline},
    error::AppError,
    harness::{AiHarness, ExecutionMode, HarnessOutcome, LiveProgress, PlanningRequest},
};
use std::time::Instant;

pub(super) fn model_batch_context(contract: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "product_modules": contract["productModules"],
        "repository_bases": contract["repositoryBases"]
    })
}

pub fn generate(
    harness: &dyn AiHarness,
    request: &PlanningRequest,
    state: &PlannerState,
    started: Instant,
) -> Result<HarnessOutcome, AppError> {
    let run = Run::open(request, state, started)?;
    let brief = state
        .workflow
        .brief
        .as_ref()
        .ok_or_else(|| AppError::Other("No approved interview brief.".into()))?;
    let task_routing = state
        .active_feature
        .as_ref()
        .map(|(id, body)| {
            let uid = crate::domain::ArtifactIdentity::from_markdown(body)
                .ok()
                .flatten()
                .map(|identity| identity.uid);
            crate::core::planning_work::routing_for_feature(&state.repo_root, id, uid.as_deref())
                .map_err(|error| AppError::Other(format!("Cannot load task routing: {error}")))
        })
        .transpose()?
        .unwrap_or_default();
    let base = format!(
        "=== APPLICATION TURN MODE ===\nGENERATE TASK STORIES. The user explicitly approved the current reviewed specification.\n\n=== APPROVED BRIEF ===\n{}\n\n=== APPROVED FEATURE SPECIFICATION ===\n{}\n\n=== FROZEN AFFECTED PRODUCT MODULES AND REPOSITORY BASES ===\n{}\n",
        serde_json::to_string_pretty(brief).unwrap_or_default(),
        state.planning_contract().unwrap_or_default(),
        serde_json::to_string_pretty(&model_batch_context(&run.identity["batch_contract"]))
            .unwrap_or_default()
    );
    let mut cp = if let Some(cp) = run.load(state) {
        let _ = request.progress_tx.send(LiveProgress {
            response: format!(
                "Resuming task generation: {} of {} detailed stories already validated.",
                cp.stories.len(),
                cp.outline.len()
            ),
            ..Default::default()
        });
        cp
    } else {
        let mut outline_request = request.clone();
        outline_request.mode = ExecutionMode::ReadOnlyAnalysis;
        let outline = run.validated(
            harness,
            &outline_request,
            "Task outline",
            OUTLINE_ATTEMPTS,
            format!("{base}\n{}\n", TASK_OUTLINE_STEP),
            |text| {
                let outline = decode_outline(text)?;
                validate_outline(brief, &outline)?;
                let manifest = &state.repositories;
                for task in &outline {
                    if manifest.repositories.len() > 1 && task.target_repository.is_empty() {
                        return Err(vec![
                            "Each multi-repository outline task needs target_repository".into(),
                        ]);
                    }
                    let id = if task.target_repository.is_empty() {
                        "root"
                    } else {
                        task.target_repository.as_str()
                    };
                    if !manifest.repositories.iter().any(|repo| repo.id == id) {
                        return Err(vec![format!("Unknown target_repository {id}")]);
                    }
                }
                Ok(outline)
            },
        )?;
        let cp = Checkpoint {
            identity: run.identity.clone(),
            outline,
            stories: Vec::new(),
        };
        run.write("checkpoint", &cp)?;
        cp
    };
    let publish = |cp: &Checkpoint| -> anyhow::Result<()> {
        anyhow::ensure!(
            crate::artifacts::spec_doc::load(&state.repo_root)? == state.spec_text,
            "Specification changed during generation"
        );
        anyhow::ensure!(
            crate::artifacts::product_docs::active_feature_for_workflow(
                &state.repo_root,
                &state.workflow,
            ) == state.active_feature,
            "Active feature changed during generation"
        );
        anyhow::ensure!(
            serde_json::to_value(crate::core::contract_snapshot::freeze(state)?)?
                == run.identity["batch_contract"],
            "Affected product modules or repository bases changed during generation"
        );
        anyhow::ensure!(
            crate::artifacts::task_docs::load_workflow(&state.repo_root)? == state.workflow,
            "Interview changed during generation"
        );
        if let Some((id, body)) = &state.active_feature {
            let uid = crate::domain::ArtifactIdentity::from_markdown(body)
                .ok()
                .flatten()
                .map(|identity| identity.uid);
            anyhow::ensure!(
                crate::core::planning_work::routing_for_feature(
                    &state.repo_root,
                    id,
                    uid.as_deref()
                )? == task_routing,
                "Task routing changed during generation"
            );
        }
        let batch = crate::core::workflow::TaskBatch {
            brief: brief.clone(),
            specification: state.planning_contract().unwrap_or_default().to_string(),
            feature_id: state.active_feature.as_ref().map(|(id, _)| id.clone()),
            contract: crate::core::contract_snapshot::freeze(state)?,
            branch_targets: state
                .active_feature
                .as_ref()
                .and_then(|(id, _)| state.workflow.feature_branch_targets.get(id).cloned()),
            task_routing: task_routing.clone(),
            stories: cp.stories.clone(),
        };
        // Defense in depth at publish: the batch must carry one consistent
        // feature identity across brief, frozen specification and stamp. The
        // prepare-stage guard covers fresh turns; this covers resumes and any
        // refactor that loosens the front door.
        let stamped = batch.feature_id.clone();
        let declared = crate::core::workflow::feature_ids_in(&batch.brief.feature_name);
        if let Some(problem) =
            crate::core::workflow::brief_target_problem(&declared, stamped.as_deref(), &|_| true)
        {
            anyhow::bail!("{problem}");
        }
        if let (Some(spec_id), Some(stamped)) = (
            specification_h1_feature_id(&batch.specification),
            stamped.as_ref(),
        ) && spec_id.as_str() != stamped
        {
            anyhow::bail!(
                "Batch specification is the {spec_id} document, but the batch is stamped {stamped}; activate {spec_id} as the active feature and regenerate its stories"
            );
        }
        crate::artifacts::task_docs::save_progress(
            &state.repo_root,
            &format!(
                "{:016x}",
                crate::persistence::fnv1a64(&serde_json::to_vec(&run.identity).unwrap_or_default())
            ),
            &batch,
            cp.outline.len(),
        )?;
        Ok(())
    };
    publish(&cp)?;
    for i in cp.stories.len()..cp.outline.len() {
        run.remaining(request)?;
        let planned = &cp.outline[i];
        let dependencies: Vec<_> = planned
            .dependencies
            .iter()
            .map(|n| &cp.stories[n - 1])
            .collect();
        let prompt = format!(
            "{base}\n=== APPLICATION GENERATION STEP ===\nWrite ONLY detailed story {} of {}. Return one complete task_stories entry. The application supplies its stable title, purpose and reference mappings from the outline; concentrate on the implementation detail. Preserve the approved scope.\n=== APPROVED TASK OUTLINE ===\n{}\n=== CURRENT TASK ===\n{}\n=== COMPLETED DEPENDENCY STORIES ===\n{}\nReuse these interfaces and decisions; do not invent conflicting contracts.\n{}",
            i + 1,
            cp.outline.len(),
            serde_json::to_string_pretty(&cp.outline).unwrap_or_default(),
            serde_json::to_string_pretty(planned).unwrap_or_default(),
            serde_json::to_string_pretty(&dependencies).unwrap_or_default(),
            STORY_CONTRACT
        );
        let story = run.validated(
            harness,
            request,
            &format!("Task {} of {}: {}", i + 1, cp.outline.len(), planned.title),
            STORY_ATTEMPTS,
            prompt,
            |text| story_response(text, planned, i),
        )?;
        cp.stories.push(story);
        run.write("checkpoint", &cp)?;
        publish(&cp)?;
    }
    run.remaining(request)?;
    let final_text = serde_json::json!({"schema_version":1,"assistant_message":format!("Prepared {} implementation-ready task stories, each validated for detail, dependencies and approved scope.", cp.stories.len()),"task_stories":cp.stories}).to_string();
    Ok(HarnessOutcome {
        final_text,
        envelope: None,
        stderr_tail: String::new(),
    })
}
