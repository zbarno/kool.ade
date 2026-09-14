//! Resumable task generation with per-artifact validation and bounded repair.
//! Checkpoints and immutable attempt evidence are private runtime data, not tasks.
use crate::{
    core::{
        state::PlannerState,
        workflow::{TaskOutline, TaskStory, story_detail_errors, validate_outline},
    },
    error::AppError,
    harness::{AiHarness, HarnessOutcome, LiveProgress, PlanningRequest, TurnEnvelope},
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
        let mut file = options.open(path)?;
        file.write_all(
            &serde_json::to_vec_pretty(value).map_err(|e| AppError::Other(e.to_string()))?,
        )?;
        file.sync_all()?;
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

fn decode(text: &str) -> Result<TurnEnvelope, Vec<String>> {
    let object = crate::harness::pi_extract::extract_json_object(text).ok_or_else(|| vec!["Response is incomplete or missing its JSON object. Return one complete object with the requested fields, including closing braces.".to_owned()])?;
    serde_json::from_str(&object).map_err(|e| {
        vec![format!(
            "Invalid task response schema: {e}. Use exactly the requested field names and types."
        )]
    })
}
fn unchanged(env: &TurnEnvelope) -> Result<(), Vec<String>> {
    if env.schema_version.is_some_and(|v| v != 1)
        || env.updated_specification.is_some()
        || env.interview.is_some()
        || !env.added().is_empty()
        || !env.updated().is_empty()
        || !env.resolved().is_empty()
    {
        return Err(vec!["Task generation may not modify the specification, interview, or open items. Omit these fields or use null/empty arrays; schema_version must be 1.".into()]);
    }
    Ok(())
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
    let mut env = decode(text)?;
    unchanged(&env)?;
    let mut stories = env.task_stories.take().unwrap_or_default();
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
        let outline = run.validated(harness, request, "Task outline", format!("{base}\n{}\nUse descriptive action + component + behavior titles, 4 or more words and at most 140 characters. Avoid generic phases such as Setup, Foundation, Implementation or Testing. Give each purpose at least 8 words explaining the specific problem this ticket solves and why it matters.\n", crate::core::prompt::TASK_OUTLINE_STEP), |text| {
            let env = decode(text)?; unchanged(&env)?;
            let outline = env.task_outline.unwrap_or_default();
            validate_outline(brief, &outline)?;
            let manifest = &state.repositories;
            for task in &outline {
                if manifest.repositories.len() > 1 && task.target_repository.is_empty() {
                    return Err(vec!["Each multi-repository outline task needs target_repository".into()]);
                }
                let id = if task.target_repository.is_empty() { "root" } else { task.target_repository.as_str() };
                if !manifest.repositories.iter().any(|repo| repo.id == id) {
                    return Err(vec![format!("Unknown target_repository {id}")]);
                }
            }
            if outline.iter().any(|o| o.purpose.split_whitespace().count() < 8) { return Err(vec!["Every task purpose needs at least 8 words describing the specific problem this ticket solves and why it matters.".into()]); }
            Ok(outline)
        })?;
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
A story must be a self-contained implementation brief, not a checklist of vague headings.
Ticket intent answers: What specific problem does this ticket solve, and why is solving
that problem necessary? Ticket goal answers: What becomes possible or reliable when
this ticket alone is complete? Explain the current limitation and its consequences.
For persistence, explain that records currently disappear on restart and why losing
those records matters; the goal is durable round-trip storage, not the entire search
experience. For a picker, explain that stored records cannot yet be selected in the
workflow; the goal is selecting and applying a record through the existing query path.
Use repository evidence for current-behavior claims and label unverified assumptions.
Do not copy the feature goal into every ticket or reuse a generic rationale across tasks.
Use the following JSON shape (snake_case or camelCase accepted):
{"task_stories":[{
"intent":"The specific current gap, failure or pain THIS ticket addresses, who or what it affects, and why leaving it unresolved matters. Explain the causal reason this ticket is needed, not the overall product mission (15+ words)",
"goal":"The specific before-to-after behavior achieved by THIS ticket alone and an observable completion condition. Do not claim benefits that require later tickets (12+ words)",
"context":"Current behavior, existing/proposed components, required inputs, example data, constraints and assumptions established from repository inspection (45+ words)",
"user_story":"As a specific user, I want a concrete behavior so that an explicit outcome is achieved (12+ words)",
"technical_design":["3+ design entries, each 12+ words: exact interfaces, signatures, data shapes, events/state changes, persistence or error contracts as applicable"],
"affected_files":["Existing or proposed path/component: specific responsibility and edits, 6+ words per entry"],
"implementation_steps":["5+ ordered steps, each 12+ words: where to change code, what to implement, how to integrate and preserve compatibility"],
"acceptance_criteria":["4+ distinct Given/When/Then criteria, each 12+ words; explicit inputs and expected observable behavior, including failure cases"],
"edge_cases":["3+ distinct cases, each 12+ words: trigger, required behavior and preserved invariants; choose relevant cases rather than generic filler"],
"test_plan":["4+ test descriptions, each 12+ words: fixture/setup, operation, exact assertions, negative paths and meaningful runtime checks"],
"verification_commands":["At least one verified command with working directory and expected result; if the repo has no runner, explicitly describe the prerequisite to establish and exact verification procedure instead of inventing one (6+ words)"],
"rollout_notes":"Compatibility, initialization/migration, rollout and rollback details or why they do not apply (20+ words)",
"definition_of_done":["3+ distinct completion checks, each 8+ words: actual evidence required, complete integration, no unresolved requirements"]
}]}
Provide 450+ words of task-specific implementation detail overall; typically 700-1400 words
is appropriate. Counts are minimum depth checks, NOT permission to pad or repeat boilerplate.
Resolve design details within the agreed scope using repository evidence. Include relevant
empty/invalid states, repeated requests, permission failures, concurrency, cancellation,
recovery and compatibility cases when they apply. Name the specific invariants and outputs.
A small local model must not have to guess file responsibilities, interfaces, test assertions,
what is in/out of scope, or how to decide completion. Do not use TODO/TBD placeholders.
Do not re-explain the entire project; focus this detail on the current task.
"#;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scope_changes_are_rejected_but_wording_and_reference_order_are_stable() {
        let outlines = decode(include_str!("../../tests/fixtures/task-outline.json"))
            .unwrap()
            .task_outline
            .unwrap();
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
        let outlines = decode(include_str!("../../tests/fixtures/task-outline.json"))
            .unwrap()
            .task_outline
            .unwrap();
        let errors = story_response(r#"{"task_stories":[{}]}"#, &outlines[0], 0)
            .unwrap_err()
            .join(" ");
        for field in [
            "intent",
            "goal",
            "context",
            "acceptance",
            "edge",
            "verification",
        ] {
            assert!(
                errors.contains(field),
                "missing repair guidance for {field}: {errors}"
            );
        }
        assert!(decode(r#"{"task_stories":["#).is_err());
    }
}
