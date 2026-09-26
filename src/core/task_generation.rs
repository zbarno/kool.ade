//! Resumable task generation with per-artifact validation and bounded repair.
//! Checkpoints and immutable attempt evidence are private runtime data, not tasks.
use crate::{
    core::{
        state::PlannerState,
        workflow::{TaskOutline, TaskStory, story_detail_errors, validate_outline},
    },
    error::AppError,
    harness::{
        AiHarness, HarnessOutcome, LiveProgress, PlanningRequest,
        responses::{self, TaskOutlineResponse, TaskStoryResponse},
    },
};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::atomic::Ordering, time::Instant};

const ATTEMPTS: usize = 3;
const CONTRACT_VERSION: u32 = 3;

#[derive(Serialize, Deserialize)]
struct Checkpoint {
    identity: serde_json::Value,
    outline: Vec<TaskOutline>,
    stories: Vec<TaskStory>,
}

struct Run {
    dir: PathBuf,
    identity: serde_json::Value,
    started: Instant,
}
impl Run {
    fn open(
        request: &PlanningRequest,
        state: &PlannerState,
        started: Instant,
    ) -> anyhow::Result<Self> {
        let delta = std::process::Command::new("git")
            .args(["diff", "--no-ext-diff", "--binary", "HEAD"])
            .current_dir(&state.repo_root)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| crate::persistence::fnv1a64(&o.stdout));
        let batch_contract = crate::core::contract_snapshot::freeze(state)?;
        let identity = serde_json::json!({"contract":CONTRACT_VERSION, "workflow":state.workflow,
            "specification":state.planning_contract(), "head":crate::core::gitops::snapshot(&state.repo_root).head_short,
            "working_tree":delta, "system":SYSTEM, "configuration":crate::artifacts::config_io::serialize(&state.config),
            "batch_contract":batch_contract});
        let encoded = serde_json::to_vec(&identity).map_err(|e| AppError::Other(e.to_string()))?;
        let repo = state
            .repo_root
            .canonicalize()
            .unwrap_or_else(|_| state.repo_root.clone());
        let dir = crate::persistence::project_dir(&crate::persistence::project_slug(&repo))
            .join("task-generation")
            .join(format!("{:016x}", crate::persistence::fnv1a64(&encoded)));
        std::fs::create_dir_all(&dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        }
        let _ = request;
        Ok(Self {
            dir,
            identity,
            started,
        })
    }
    fn write(&self, label: &str, value: &impl Serialize) -> anyhow::Result<()> {
        use std::io::Write;
        let path = self.dir.join(format!(
            "{}-{}-{label}.json",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default(),
            std::process::id()
        ));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path)?;
        file.write_all(
            &serde_json::to_vec_pretty(value).map_err(|e| AppError::Other(e.to_string()))?,
        )?;
        file.sync_all()?;
        crate::artifacts::sync_parent_directory(&path)?;
        Ok(())
    }
    fn load(&self, state: &PlannerState) -> Option<Checkpoint> {
        let brief = state.workflow.brief.as_ref()?;
        let mut files: Vec<_> = std::fs::read_dir(&self.dir)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .is_some_and(|s| s.to_string_lossy().ends_with("-checkpoint.json"))
            })
            .collect();
        files.sort();
        for path in files.into_iter().rev() {
            let Some(cp) = std::fs::read(path)
                .ok()
                .and_then(|data| serde_json::from_slice::<Checkpoint>(&data).ok())
            else {
                continue;
            };
            if cp.identity != self.identity
                || cp.stories.len() > cp.outline.len()
                || validate_outline(brief, &cp.outline).is_err()
            {
                continue;
            }
            if cp.stories.iter().enumerate().all(|(i, story)| {
                story_detail_errors(story, i + 1).is_empty()
                    && story.title == cp.outline[i].title
                    && story.purpose == cp.outline[i].purpose
                    && story.target_repository == cp.outline[i].target_repository
                    && same_refs(&story.scope_items, &cp.outline[i].scope_items)
                    && same_refs(&story.success_criteria, &cp.outline[i].success_criteria)
                    && same_refs(&story.dependencies, &cp.outline[i].dependencies)
            }) {
                return Some(cp);
            }
        }
        None
    }
    fn remaining(&self, req: &PlanningRequest) -> Result<std::time::Duration, AppError> {
        if req.cancel.load(Ordering::SeqCst) {
            return Err(AppError::HarnessFailed {
                reason: "cancelled by user; completed stories are checkpointed for retry".into(),
                stderr_tail: String::new(),
            });
        }
        req.timeout
            .checked_sub(self.started.elapsed())
            .filter(|d| !d.is_zero())
            .ok_or(AppError::HarnessTimedOut {
                secs: req.timeout.as_secs(),
            })
    }
    fn validated<T>(
        &self,
        harness: &dyn AiHarness,
        req: &PlanningRequest,
        stage: &str,
        prompt: String,
        validate: impl Fn(&str) -> Result<T, Vec<String>>,
    ) -> Result<T, AppError> {
        let mut feedback = Vec::new();
        let mut previous = String::new();
        for attempt in 1..=ATTEMPTS {
            let mut request = req.clone();
            request.timeout = self.remaining(req)?;
            request.system_instructions = SYSTEM.into();
            request.prompt_body = prompt.clone();
            if !feedback.is_empty() {
                request.prompt_body.push_str(&format!("\n=== REPAIR THIS RESPONSE ===\nThe previous response was rejected for:\n- {}\nReturn a COMPLETE replacement JSON object, not a continuation fragment. Retain valid details, correct all listed issues, and omit commentary. Do not repeat repository exploration unless a listed issue requires it.\n=== PREVIOUS RESPONSE (may be truncated) ===\n{}\n", feedback.join("\n- "), previous.chars().take(60_000).collect::<String>()));
            }
            let _ = req.progress_tx.send(LiveProgress {
                response: format!("{stage} — attempt {attempt}/{ATTEMPTS}"),
                ..Default::default()
            });
            let outcome = harness.execute(&request);
            self.write(&format!("{}-attempt-{attempt}", crate::artifacts::task_docs::slug(stage)), &serde_json::json!({
                "stage":stage,"attempt":attempt,"timeout_seconds":request.timeout.as_secs(),"prompt":request.prompt_body,
                "response":outcome.as_ref().ok().map(|o| &o.final_text),
                "stderr":outcome.as_ref().ok().map(|o| &o.stderr_tail),
                "error":outcome.as_ref().err().map(|e| e.detail())
            }))?;
            self.remaining(req)?;
            match outcome {
                Ok(out) => {
                    previous = out.final_text;
                    match validate(&previous) {
                        Ok(value) => return Ok(value),
                        Err(problems) => feedback = problems,
                    }
                }
                // Completed-but-empty output can be repaired. Runtime/connection
                // failures preserve checkpoints and remain actionable failures.
                Err(AppError::HarnessFailed { reason, .. })
                    if reason.contains("no final assistant message") =>
                {
                    feedback = vec![
                        "No final text was returned. Emit the requested complete JSON object."
                            .into(),
                    ];
                    previous.clear();
                }
                Err(error) => return Err(error),
            }
            self.write(
                "validation",
                &serde_json::json!({"stage":stage,"attempt":attempt,"problems":feedback}),
            )?;
        }
        Err(AppError::InvalidResponse {
            problems: vec![format!(
                "{stage} still needs correction after {ATTEMPTS} attempts: {}. Completed stories are checkpointed; use Generate task stories to resume. Details: {}",
                feedback.join("; "),
                self.dir.display()
            )],
        })
    }
}

fn decode_outline(text: &str) -> Result<Vec<TaskOutline>, Vec<String>> {
    let response = responses::decode::<TaskOutlineResponse>(text).map_err(|error| {
        vec![format!(
            "Invalid task outline schema: {error}. Return only task_outline and an empty or null task_stories field."
        )]
    })?;
    responses::normalize_task_outline(response).map_err(|error| vec![error])
}
fn decode_stories(text: &str) -> Result<Vec<TaskStory>, Vec<String>> {
    let response = responses::decode::<TaskStoryResponse>(text).map_err(|error| {
        vec![format!(
            "Invalid task story schema: {error}. Return only one task_stories entry."
        )]
    })?;
    responses::normalize_task_story(response).map_err(|error| vec![error])
}
fn same_refs(a: &[usize], b: &[usize]) -> bool {
    a.iter().copied().collect::<std::collections::BTreeSet<_>>() == b.iter().copied().collect()
}
/// The app owns stable identifiers and wording from the accepted outline.
/// Omitted references are filled in; actual scope/dependency changes are repaired.
fn story_response(
    text: &str,
    planned: &TaskOutline,
    index: usize,
) -> Result<TaskStory, Vec<String>> {
    let mut stories = decode_stories(text)?;
    if stories.len() != 1 {
        return Err(vec![
            "Return exactly one complete story in task_stories.".into(),
        ]);
    }
    let mut story = stories.remove(0);
    if !story.target_repository.is_empty() && story.target_repository != planned.target_repository {
        return Err(vec![
            "target_repository changed the approved outline repository".into(),
        ]);
    }
    for (name, values, expected) in [
        ("scope_items", &story.scope_items, &planned.scope_items),
        (
            "success_criteria",
            &story.success_criteria,
            &planned.success_criteria,
        ),
        ("dependencies", &story.dependencies, &planned.dependencies),
    ] {
        if !values.is_empty() && !same_refs(values, expected) {
            return Err(vec![format!(
                "{name} changed the task's approved mapping. Expected {expected:?}; received {values:?}."
            )]);
        }
    }
    story.title = planned.title.clone();
    story.purpose = planned.purpose.clone();
    story.target_repository = planned.target_repository.clone();
    story.scope_items = planned.scope_items.clone();
    story.success_criteria = planned.success_criteria.clone();
    story.dependencies = planned.dependencies.clone();
    let errors = story_detail_errors(&story, index + 1);
    if errors.is_empty() {
        Ok(story)
    } else {
        Err(errors)
    }
}

fn specification_h1_feature_id(spec: &str) -> Option<String> {
    spec.lines()
        .find(|line| line.starts_with('#'))
        .and_then(|line| {
            crate::core::workflow::feature_ids_in(line)
                .into_iter()
                .next()
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
    let base = format!(
        "=== APPLICATION TURN MODE ===\nGENERATE TASK STORIES. The user explicitly approved the current reviewed specification.\n\n=== APPROVED BRIEF ===\n{}\n\n=== APPROVED FEATURE SPECIFICATION ===\n{}\n\n=== FROZEN AFFECTED PRODUCT MODULES AND REPOSITORY BASES ===\n{}\n",
        serde_json::to_string_pretty(brief).unwrap_or_default(),
        state.planning_contract().unwrap_or_default(),
        serde_json::to_string_pretty(&run.identity["batch_contract"]).unwrap_or_default()
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
        let outline = run.validated(
            harness,
            request,
            "Task outline",
            format!("{base}\n{}\n", crate::core::prompt::TASK_OUTLINE_STEP),
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
            crate::artifacts::product_docs::active_feature(&state.repo_root)
                == state.active_feature,
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
        let batch = crate::core::workflow::TaskBatch {
            brief: brief.clone(),
            specification: state.planning_contract().unwrap_or_default().to_string(),
            feature_id: state.active_feature.as_ref().map(|(id, _)| id.clone()),
            contract: crate::core::contract_snapshot::freeze(state)?,
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

const SYSTEM: &str = "You are Packet's implementation-story author. First understand WHY the approved product exists. Write self-contained stories that a local coding model can implement without this conversation. You may inspect repository files using read-only tools. NEVER create, modify or delete files or run mutating commands; the application writes validated output. Do not implement the tasks. Use the approved brief and specification as authority. Distinguish existing files and interfaces from proposed ones; never present guessed paths or commands as verified. Follow the current generation step, return a complete JSON object in a json fence, and stop. Do not include an interview or changes to the specification/open items. Correct prior validation errors when repair feedback is supplied.";

pub const STORY_CONTRACT: &str = r#"
Write a self-contained implementation brief for this task. Make its detail fit the
actual change: keep a small, low-risk task concise, and add detail when this issue
needs design, migration, security, compatibility, failure handling, concurrency or
recovery decisions. Do not follow fixed word counts or fixed numbers of entries.
Never add generic boilerplate or repeat information to make a story longer.

Explain the specific current gap and why it matters in intent. State the observable
before-to-after result of this task alone in goal. Use repository evidence for current
behavior; label uncertainty and include a discovery step instead of guessing. Do not
copy the feature goal into each task or promise work owned by a later task.

Return one task_stories entry using snake_case or camelCase field names. Keep the
following core information specific and concise:
{"task_stories":[{
"title":"Specific action and object",
"intent":"The current issue this task fixes and why it matters",
"goal":"Observable behavior delivered by this task alone",
"context":"Relevant current behavior, evidence and assumptions",
"user_story":"User, desired behavior and outcome",
"purpose":"The task-specific reason from the approved outline",
"affected_files":["Known path or component and its responsibility"],
"implementation_steps":["Ordered action needed to complete this task"],
"acceptance_criteria":["Observable result for a relevant user or system state"],
"test_plan":["Setup, action and expected assertion for a relevant test"],
"verification_commands":["Verified command and expected result, or a precise verification procedure"],
"definition_of_done":["Evidence that this task's agreed outcome is complete"],
"technical_design":[],
"edge_cases":[],
"rollout_notes":""
}]}

Include affected files or components when repository inspection makes them knowable;
otherwise name the discovery needed and avoid invented paths. Include technical design,
edge cases and rollout notes only when they matter to this issue. Leave irrelevant
optional arrays empty and irrelevant rollout notes blank. Verification may be a command
or a precise manual/runtime procedure when no suitable automated command exists.
Acceptance criteria, tests and implementation steps should cover the actual scope and
risks, not a preset count. A short task still needs enough information to implement,
accept and verify it; extra prose does not compensate for a missing goal or evidence.
Resolve relevant details from approved scope and read-only repository inspection.
Do not use TODO/TBD placeholders. Do not re-explain the entire project.
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detail_guidance_scales_to_the_issue_instead_of_word_counts() {
        assert!(STORY_CONTRACT.contains("Do not follow fixed word counts"));
        assert!(STORY_CONTRACT.contains("optional arrays empty"));
        assert!(!STORY_CONTRACT.contains("450+ words"));
        assert!(!STORY_CONTRACT.contains("700-1400 words"));
        assert!(!STORY_CONTRACT.contains("12+ words"));
    }

    #[test]
    fn scope_changes_are_rejected_but_wording_and_reference_order_are_stable() {
        let outlines =
            decode_outline(include_str!("../../tests/fixtures/task-outline.json")).unwrap();
        let planned = &outlines[0];
        let mut value: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/task-story-1.json")).unwrap();
        value["task_stories"][0]["title"] = "Different wording".into();
        value["task_stories"][0]["purpose"] = "Paraphrased purpose".into();
        let story = story_response(&value.to_string(), planned, 0).unwrap();
        assert_eq!(story.title, planned.title);
        assert_eq!(story.purpose, planned.purpose);
        value["task_stories"][0]["scope_items"] = serde_json::json!([999]);
        assert!(
            story_response(&value.to_string(), planned, 0).unwrap_err()[0]
                .contains("approved mapping")
        );
        assert!(same_refs(&[1, 2], &[2, 1]));
    }
    #[test]
    fn missing_detail_produces_specific_repair_feedback() {
        let outlines =
            decode_outline(include_str!("../../tests/fixtures/task-outline.json")).unwrap();
        let errors = story_response(r#"{"task_stories":[{}]}"#, &outlines[0], 0)
            .unwrap_err()
            .join(" ");
        for field in [
            "intent",
            "goal",
            "context",
            "affected files",
            "implementation steps",
            "acceptance",
            "test plan",
            "verification",
            "definition of done",
        ] {
            assert!(
                errors.contains(field),
                "missing repair guidance for {field}: {errors}"
            );
        }
        assert!(!errors.contains("edge cases"));
        assert!(decode_stories(r#"{"task_stories":["#).is_err());
    }
}
