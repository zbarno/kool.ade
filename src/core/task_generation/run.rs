use super::{
    prompt::{STORY_CONTRACT, SYSTEM},
    response::same_refs,
};
use crate::{
    core::{
        state::PlannerState,
        workflow::{TaskOutline, TaskStory, story_detail_errors, validate_outline},
    },
    error::AppError,
    harness::{AiHarness, LiveProgress, PlanningRequest},
};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::atomic::Ordering, time::Instant};

pub(super) const OUTLINE_ATTEMPTS: usize = 6;
pub(super) const STORY_ATTEMPTS: usize = 6;
const CONTRACT_VERSION: u32 = 20;

pub(super) fn working_tree_fingerprint(repo: &std::path::Path) -> Option<u64> {
    let output = std::process::Command::new("git")
        .args([
            "diff",
            "--no-ext-diff",
            "--binary",
            "HEAD",
            "--",
            ".",
            ":(exclude).kool-ade-packet/state/work.json",
        ])
        .current_dir(repo)
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    Some(crate::persistence::fnv1a64(&output.stdout))
}

pub(super) fn previous_response_for_repair(feedback: &[String], previous: &str) -> String {
    if feedback.iter().any(|problem| {
        problem.contains("no complete JSON story")
            || problem.contains("No complete JSON object was found")
    }) {
        "Return ONLY one complete JSON object. No preface, explanation, status, Markdown fence, or second attempt. The first non-whitespace character must be { and the last non-whitespace character must be }. Include exactly one task_stories entry matching this outline task. Keep the object below 3,000 characters: use one short sentence in required text fields, one short string per required list, and empty arrays for optional lists. Do not include code snippets, quoted phrases, or backslashes inside string values. Do not mention the repair or your reasoning."
            .into()
    } else if feedback.iter().any(|problem| {
        problem.contains("Invalid task story JSON") || problem.contains("Invalid task story schema")
    }) {
        "The previous story was malformed. Discard it completely and rebuild from the supplied schema. Return ONLY one complete JSON object with exactly one task_stories entry matching this outline task. No preface, explanation, Markdown fence, or second attempt. Keep the object below 3,000 characters: one short sentence per required text field, one short string per required list, and empty arrays for optional lists. Use single-level arrays of strings. Do not include code snippets, quoted phrases, or backslashes inside string values. Do not mention the repair or your reasoning."
            .into()
    } else if feedback
        .iter()
        .any(|problem| problem.contains("read-only planning"))
    {
        "Repository inspection reached Packet's bounded read limit. Do not call tools again; complete the response from the supplied approved context and evidence already gathered. If a detail is unknown, state it as a focused discovery step."
            .into()
    } else if feedback
        .iter()
        .any(|problem| problem.contains("Return exactly one complete story in task_stories"))
    {
        "The previous response did not contain exactly one story. Discard it completely. Return ONLY one complete JSON object with exactly one task_stories entry matching the current outline task. No preface, explanation, Markdown fence, or second attempt. Keep it below 2,500 characters with one short sentence per required text field, one short string per required list, and empty arrays for optional lists. Do not include code snippets, quoted phrases, backslashes, or another placeholder story. Do not mention your reasoning."
            .into()
    } else if feedback.iter().any(|problem| {
        problem.contains("Field budget overages") || problem.contains("complete story is")
    }) {
        "The previous story exceeded its size budgets. Discard it completely and rebuild the same single outline task. Return ONLY one complete JSON object with exactly one task_stories entry; no preface, explanation, Markdown fence, or second attempt. Keep the object below 2,500 characters: intent <=200; goal <=250; context <=250; affected_files <=250 total; implementation_steps <=500 total (at most 3 short strings); acceptance_criteria <=400 total (at most 2 short strings); test_plan <=300 total (at most 2 short strings); definition_of_done one string <=80. Use one short sentence each for user_story and purpose. Keep technical_design and edge_cases empty, rollout_notes empty, and include one concrete runnable verification command under 200 characters. Preserve only this task's approved behavior; omit speculative detail and work owned by other tasks. Do not repeat requirements. No code snippets, quoted phrases, or backslashes inside string values."
            .into()
    } else {
        previous.chars().take(60_000).collect()
    }
}

pub(super) fn repair_is_response_only(feedback: &[String]) -> bool {
    feedback.iter().any(|problem| {
        problem.contains("Field budget overages")
            || problem.contains("complete story is")
            || problem.contains("Invalid task story JSON")
            || problem.contains("No complete JSON object was found")
            || problem.contains("Return exactly one complete story in task_stories")
            || problem.contains("read-only planning")
    })
}

#[derive(Serialize, Deserialize)]
pub(super) struct Checkpoint {
    pub(super) identity: serde_json::Value,
    pub(super) outline: Vec<TaskOutline>,
    pub(super) stories: Vec<TaskStory>,
}

pub(super) struct Run {
    dir: PathBuf,
    pub(super) identity: serde_json::Value,
    started: Instant,
}
impl Run {
    pub(super) fn open(
        request: &PlanningRequest,
        state: &PlannerState,
        started: Instant,
    ) -> anyhow::Result<Self> {
        // The planning-work board is operational UI state, not evidence used
        // to author a story. Saving retry/failure status must preserve a run.
        let delta = working_tree_fingerprint(&state.repo_root);
        let batch_contract = crate::core::contract_snapshot::freeze(state)?;
        let identity = serde_json::json!({"contract":CONTRACT_VERSION, "workflow":state.workflow,
            "specification":state.planning_contract(), "head":crate::core::gitops::snapshot(&state.repo_root).head_short,
            "working_tree":delta, "system":SYSTEM, "configuration":crate::artifacts::config_io::serialize(&state.config),
            "story_contract":STORY_CONTRACT, "batch_contract":batch_contract});
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
    pub(super) fn write(&self, label: &str, value: &impl Serialize) -> anyhow::Result<()> {
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
    pub(super) fn load(&self, state: &PlannerState) -> Option<Checkpoint> {
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
    pub(super) fn remaining(&self, req: &PlanningRequest) -> Result<std::time::Duration, AppError> {
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
    pub(super) fn validated<T>(
        &self,
        harness: &dyn AiHarness,
        req: &PlanningRequest,
        stage: &str,
        max_attempts: usize,
        prompt: String,
        validate: impl Fn(&str) -> Result<T, Vec<String>>,
    ) -> Result<T, AppError> {
        let mut feedback = Vec::new();
        let mut previous = String::new();
        for attempt in 1..=max_attempts {
            let mut request = req.clone();
            request.timeout = self.remaining(req)?;
            request.system_instructions = SYSTEM.into();
            request.prompt_body = prompt.clone();
            if !feedback.is_empty() {
                let prior = previous_response_for_repair(&feedback, &previous);
                let research_rule = if repair_is_response_only(&feedback) {
                    "This is a response-only repair: do not call repository tools or gather more evidence."
                } else {
                    "Do not repeat repository exploration unless a listed issue requires it."
                };
                request.prompt_body.push_str(&format!(
                    "\n=== REPAIR THIS RESPONSE ===\nThe previous response was rejected for:\n- {}\nReturn a COMPLETE replacement JSON object, not a continuation fragment. Preserve the approved outline scope, correct every listed issue, remove unsupported details, and omit commentary. {research_rule}\n=== PREVIOUS RESPONSE OR REPAIR GUIDANCE ===\n{}\n",
                    feedback.join("\n- "), prior
                ));
            }
            let _ = req.progress_tx.send(LiveProgress {
                response: format!("{stage} — attempt {attempt}/{max_attempts}"),
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
                Err(AppError::HarnessFailed { reason, .. })
                    if reason.contains("read-only planning") =>
                {
                    feedback = vec![format!(
                        "Bounded repository inspection stopped this response before the requested tool ran: {reason}. Return the complete requested JSON using only supplied context and evidence already gathered. Do not call repository tools again; record any unverified detail as focused discovery."
                    )];
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
                "{stage} still needs correction after {max_attempts} attempts: {}. Completed stories are checkpointed; use Generate task stories to resume. Details: {}",
                feedback.join("; "),
                self.dir.display()
            )],
        })
    }
}
