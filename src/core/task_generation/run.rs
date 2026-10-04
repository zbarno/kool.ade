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

use super::repair::{
    previous_response_for_repair, repair_execution_mode, repair_is_response_only,
    working_tree_fingerprint,
};

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
            request.mode = repair_execution_mode(request.mode, &feedback);
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
