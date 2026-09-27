//! Interview readiness, explicit task-generation consent, and detailed story validation.
use crate::core::{state::PlannerState, validation::NormalizedTurn};
use crate::harness::TurnEnvelope;
use serde::{Deserialize, Serialize};

#[cfg(test)]
#[path = "workflow/contract_tests.rs"]
mod contract_tests;
mod outline_validation;
mod story_validation;
pub use outline_validation::validate_outline;
use story_validation::validate_stories;
pub use story_validation::{descriptive_title, story_detail_errors};

pub const WORKFLOW_FILE: &str = crate::artifacts::layout::canonical::WORKFLOW;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnPurpose {
    #[default]
    Interview,
    /// Refresh a stale brief for an already authorized generation action.
    ReviewForGeneration,
    GenerateTasks,
    ComparePlans,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterviewBrief {
    #[serde(alias = "feature_name")]
    pub feature_name: String,
    pub problem: String,
    pub goal: String,
    #[serde(alias = "target_users")]
    pub target_users: String,
    #[serde(alias = "intended_outcome")]
    pub intended_outcome: String,
    #[serde(alias = "success_criteria")]
    pub success_criteria: Vec<String>,
    #[serde(alias = "in_scope")]
    pub in_scope: Vec<String>,
    #[serde(alias = "out_of_scope")]
    pub out_of_scope: Vec<String>,
    pub constraints: Vec<String>,
    #[serde(alias = "ready_for_tasks")]
    pub ready_for_tasks: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workflow {
    pub brief: Option<InterviewBrief>,
    /// The exact specification the readiness assessment covers.
    pub reviewed_specification: Option<String>,
    pub task_batches: Vec<TaskBatchRef>,
    #[serde(default)]
    pub approved_features: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskBatchRef {
    /// Packet-owned immutable identity. Missing only in legacy workflow data.
    #[serde(default)]
    pub identity: Option<crate::domain::ArtifactIdentity>,
    pub feature: String,
    pub directory: String,
    pub count: usize,
}

fn default_repository() -> String {
    String::new()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TaskStory {
    pub title: String,
    #[serde(default = "default_repository", alias = "target_repository")]
    pub target_repository: String,
    pub intent: String,
    pub goal: String,
    pub context: String,
    #[serde(alias = "technical_design")]
    pub technical_design: Vec<String>,
    #[serde(alias = "verification_commands")]
    pub verification_commands: Vec<String>,
    #[serde(alias = "user_story")]
    pub user_story: String,
    pub purpose: String,
    /// One-based references into the approved brief, checked for full coverage.
    #[serde(alias = "scope_items")]
    pub scope_items: Vec<usize>,
    #[serde(alias = "success_criteria")]
    pub success_criteria: Vec<usize>,
    pub dependencies: Vec<usize>,
    #[serde(alias = "affected_files")]
    pub affected_files: Vec<String>,
    #[serde(alias = "implementation_steps")]
    pub implementation_steps: Vec<String>,
    #[serde(alias = "acceptance_criteria")]
    pub acceptance_criteria: Vec<String>,
    #[serde(alias = "test_plan")]
    pub test_plan: Vec<String>,
    #[serde(alias = "edge_cases")]
    pub edge_cases: Vec<String>,
    #[serde(alias = "rollout_notes")]
    pub rollout_notes: String,
    #[serde(alias = "definition_of_done")]
    pub definition_of_done: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TaskOutline {
    pub title: String,
    pub purpose: String,
    #[serde(alias = "target_repository")]
    pub target_repository: String,
    #[serde(alias = "scope_items")]
    pub scope_items: Vec<usize>,
    #[serde(alias = "success_criteria")]
    pub success_criteria: Vec<usize>,
    pub dependencies: Vec<usize>,
}

#[derive(Debug, Clone)]
pub struct TaskBatch {
    pub brief: InterviewBrief,
    pub specification: String,
    pub feature_id: Option<String>,
    pub contract: Option<crate::core::contract_snapshot::BatchContract>,
    pub stories: Vec<TaskStory>,
}

/// Freeze the normative feature contract, excluding mutable status and
/// repository-observation sections. A changed desired behavior needs approval again.
pub fn feature_contract(text: &str) -> String {
    let mut capture = false;
    let mut selected = String::new();
    for line in text.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            capture = [
                "Intent",
                "Desired Behavior",
                "Scope",
                "Requirements",
                "Decisions and Assumptions",
                "Acceptance Criteria",
                "Selected Plan",
            ]
            .contains(&heading);
        }
        if capture {
            selected.push_str(line);
            selected.push('\n');
        }
    }
    selected
}

pub fn approve_feature(
    repo: &std::path::Path,
    workflow: &mut Workflow,
    id: &str,
) -> anyhow::Result<String> {
    approve_feature_if_current(repo, workflow, id, None)
}

pub fn approve_feature_if_current(
    repo: &std::path::Path,
    workflow: &mut Workflow,
    id: &str,
    expected_contract: Option<&str>,
) -> anyhow::Result<String> {
    // Writer section: this read-modify-commit of workflow.json shares the
    // planning index with background turns/reconciliation.
    let guard = crate::core::writer_gate::acquire();
    let path = crate::artifacts::product_docs::document_path(repo, &format!("feature:{id}"))?;
    let text = std::fs::read_to_string(path)?;
    crate::core::specification::validate_feature(id, &text)?;
    let metadata = crate::domain::ChangeMetadata::require_markdown(&text)?;
    let status = metadata.status;
    anyhow::ensure!(
        status.approval_eligible(),
        "Only a ready or already implementing feature may be approved"
    );
    anyhow::ensure!(
        metadata.schema_version != 2 || metadata.selected_alt.is_some(),
        "Compare and adopt a plan before approving this feature"
    );
    let contract = feature_contract(&text);
    anyhow::ensure!(!contract.trim().is_empty(), "Feature contract is empty");
    anyhow::ensure!(
        expected_contract.is_none_or(|expected| expected == contract),
        "The feature changed since it was displayed. Refresh and review its current specification."
    );
    // Another conversation may have saved a brief or another approval since display.
    *workflow = crate::artifacts::task_docs::load_workflow(repo)?;
    workflow.approved_features.insert(id.to_string(), contract);
    crate::artifacts::task_docs::save_workflow(repo, workflow)?;
    let result = crate::core::gitops::commit(
        repo,
        &format!("planner: approve feature {id}"),
        &[WORKFLOW_FILE.to_string()],
    )
    .map_err(|e| anyhow::anyhow!("approval saved but checkpoint failed: {e}"));
    drop(guard);
    result
}

pub fn feature_approved(repo: &std::path::Path, workflow: &Workflow, id: &str) -> bool {
    let Ok(path) = crate::artifacts::product_docs::document_path(repo, &format!("feature:{id}"))
    else {
        return false;
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(metadata) = crate::domain::ChangeMetadata::require_markdown(&text) else {
        return false;
    };
    if metadata.schema_version == 2 && metadata.selected_alt.is_none() {
        return false;
    }
    workflow
        .approved_features
        .get(id)
        .is_some_and(|snapshot| snapshot == &feature_contract(&text))
}

impl Workflow {
    pub fn ready(&self, specification: Option<&str>) -> bool {
        self.brief.as_ref().is_some_and(|b| b.ready_for_tasks)
            && specification.is_some_and(|s| !s.trim().is_empty())
            && self.reviewed_specification.as_deref() == specification
    }
}

/// Scan `text` for stable `F<number>` and legacy `CHG-nnn` feature
/// identifiers, de-duplicated and sorted. Prose that embeds no id (legacy MVP
/// batches) yields an empty list and grandfathers through identity checks.
pub fn feature_ids_in(text: &str) -> Vec<String> {
    let mut ids = std::collections::BTreeSet::new();
    for token in text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-')) {
        if crate::artifacts::product_docs::valid_feature_id(token) {
            ids.insert(token.to_string());
        }
    }
    ids.into_iter().collect()
}

/// Judge a brief's declared feature identity (`declared`, de-duplicated)
/// against the feature the batch would actually be stamped with (`stamped`,
/// the active feature id). `feature_dir_exists` tests whether a candidate id
/// has a feature document. Returns an actionable operator message when the
/// two identities cannot both be honored; `None` means the guard passes.
pub fn brief_target_problem(
    declared: &[String],
    stamped: Option<&str>,
    feature_dir_exists: &dyn Fn(&str) -> bool,
) -> Option<String> {
    match declared {
        [] => None,
        [id] if !feature_dir_exists(id) => Some(format!(
            "Brief references unknown feature {id}; record the change specification under .kool-ade-packet/planning/changes before generating tasks"
        )),
        [id] => match stamped {
            Some(stamped) if *id == stamped => None,
            Some(stamped) => Some(format!(
                "Brief targets {id}, but the active feature is {stamped}; generation stamps every story with {stamped} and freezes its specification — make {id} the active feature (conclude {stamped} first, or reopen {id} if it was concluded) before generating its tasks"
            )),
            None => Some(format!(
                "Brief references feature {id}, but no feature is active"
            )),
        },
        _ => Some(format!(
            "Brief feature name declares more than one feature id ({}); name exactly one",
            declared.join(", ")
        )),
    }
}

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
                    nt.assistant_message.push_str(&format!("\n\nThe goal and scope for {} are ready to break down. Use the Generate task stories action when you want Packet to prepare the work, or keep refining the plan.", brief.feature_name));
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

fn substantive(text: &str) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{apply, validation};

    fn brief() -> InterviewBrief {
        InterviewBrief {
            feature_name: "Saved searches".into(),
            problem: "Analysts repeat the same query configuration every morning.".into(),
            goal: "Let analysts resume a useful query without reconstructing it.".into(),
            target_users: "Analysts monitoring daily operational changes.".into(),
            intended_outcome: "Select a saved query and see current matching results.".into(),
            success_criteria: vec![
                "A saved query can be restored after restarting the app.".into(),
            ],
            in_scope: vec!["Persist and restore named search filters.".into()],
            out_of_scope: vec!["Sharing saved searches across users.".into()],
            constraints: vec!["Keep existing ad hoc search behavior compatible.".into()],
            ready_for_tasks: true,
        }
    }
    fn story() -> TaskStory {
        let response: crate::harness::responses::TaskStoryResponse =
            serde_json::from_str(include_str!("../../tests/fixtures/task-story-1.json")).unwrap();
        let mut story = response.task_stories.unwrap().remove(0);
        story.success_criteria = vec![1];
        story
    }

    #[test]
    fn small_task_needs_no_padding_and_incomplete_meaning_stays_invalid() {
        let small = TaskOutline {
            title: "Fix crash".into(),
            purpose: "Opening a saved filter crashes.".into(),
            scope_items: vec![1],
            success_criteria: vec![1],
            ..Default::default()
        };
        assert!(validate_outline(&brief(), &[small]).is_ok());

        let missing_references = TaskOutline {
            title: "Prepare the task batch".into(),
            purpose: "Separate the approved scope into implementable work.".into(),
            ..Default::default()
        };
        let feedback = validate_outline(&brief(), &[missing_references])
            .unwrap_err()
            .join(" ");
        assert!(feedback.contains("scope item index(es) [1]"));
        assert!(feedback.contains("success criterion index(es) [1]"));

        let mut concise = story();
        concise.title = "Show full blocker".into();
        concise.intent = "Long blocker reports hide the choices the user needs.".into();
        concise.goal = "Show the full report and let the user choose a response.".into();
        concise.context = "Task details currently clip long blocker reports.".into();
        concise.user_story =
            "As a blocked user, I need the whole issue and its reply control.".into();
        concise.purpose = "Restore the context needed to choose what happens next.".into();
        concise.affected_files = vec!["src/app/task_detail.rs: show the complete blocker".into()];
        concise.implementation_steps =
            vec!["Render the full report and its matching response control.".into()];
        concise.acceptance_criteria =
            vec!["A long blocker displays its full explanation and available choices.".into()];
        concise.test_plan =
            vec!["Render a long blocker and assert all text and choices remain visible.".into()];
        concise.verification_commands = vec!["cargo test --offline --lib task_detail".into()];
        concise.definition_of_done =
            vec!["The complete issue is visible and the user can submit a response.".into()];
        concise.technical_design.clear();
        concise.edge_cases.clear();
        concise.rollout_notes.clear();
        assert!(story_detail_errors(&concise, 1).is_empty());

        concise.intent.clear();
        concise.context = "Padding that does not explain the actual blocker. ".repeat(100);
        let errors = story_detail_errors(&concise, 1);
        assert!(
            errors
                .iter()
                .any(|error| error.contains("task-specific intent"))
        );
    }

    fn state(tag: &str) -> PlannerState {
        let root =
            std::env::temp_dir().join(format!("packet_workflow_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        let mut s = PlannerState::load(&root).unwrap();
        s.bootstrap_missing().unwrap();
        s
    }
    fn envelope() -> TurnEnvelope {
        serde_json::from_value(serde_json::json!({"assistant_message":"The goal is to restore analysts' daily queries without repeating setup."})).unwrap()
    }
    fn mark_ready(state: &mut PlannerState) {
        let mut env = envelope();
        env.interview = Some(brief());
        let nt = validation::validate(&env, state, &state.effective_user()).unwrap();
        assert!(
            nt.assistant_message
                .contains("Generate task stories action")
        );
        apply::apply(state, &nt).unwrap();
    }
    fn generation(
        state: &PlannerState,
        stories: Vec<TaskStory>,
    ) -> Result<NormalizedTurn, Vec<String>> {
        let mut env = envelope();
        env.task_stories = Some(stories);
        validation::validate_for_turn(
            &env,
            state,
            &state.effective_user(),
            TurnPurpose::GenerateTasks,
        )
    }

    #[test]
    fn readiness_persists_and_approved_stories_are_detailed_and_numbered() {
        let mut s = state("complete");
        mark_ready(&mut s);
        s = PlannerState::load(&s.repo_root).unwrap();
        assert!(s.workflow.ready(s.spec_text.as_deref()));
        assert!(
            !s.repo_root.join(".kool-ade-packet/planning/tasks").exists(),
            "readiness must not generate files"
        );
        let mut second = story();
        second.title = "Connect the saved search picker".into();
        second.dependencies = vec![1];
        let nt = generation(&s, vec![story(), second]).unwrap();
        let receipt = apply::apply(&mut s, &nt).unwrap();
        let dir = s
            .repo_root
            .join(".kool-ade-packet/planning/tasks/saved-searches");
        let task =
            std::fs::read_to_string(dir.join("001-persist-named-search-filters.md")).unwrap();
        assert!(task.contains(&story().intent));
        assert!(task.contains(&story().goal));
        assert!(!task.contains("## Product goal and intent"));
        assert!(!task.contains(&s.workflow.brief.as_ref().unwrap().goal));
        for section in [
            "Problem this ticket solves and why",
            "Ticket goal — what changes when done",
            "User story",
            "Implementation steps",
            "Acceptance criteria",
            "Test plan",
            "Edge cases",
            "Rollout",
            "Definition of done",
        ] {
            assert!(task.contains(section));
        }
        let second =
            std::fs::read_to_string(dir.join("002-connect-the-saved-search-picker.md")).unwrap();
        assert!(second.contains("[Task 001](001-persist-named-search-filters.md)"));
        assert_eq!(
            std::fs::read_to_string(dir.join("specification.md")).unwrap(),
            s.spec_text.clone().unwrap()
        );
        assert!(receipt.repo_relative_paths.contains(&WORKFLOW_FILE.into()));
        let restored = PlannerState::load(&s.repo_root).unwrap();
        assert!(!restored.workflow.ready(restored.spec_text.as_deref()));
        assert_eq!(
            crate::artifacts::task_docs::load_latest(&restored.repo_root, &restored.workflow).len(),
            2
        );
        assert!(
            generation(&restored, vec![story()]).is_err(),
            "approval is consumed after generation"
        );
        std::fs::remove_dir_all(s.repo_root).unwrap();
    }

    #[test]
    fn no_approval_or_incomplete_intent_cannot_generate_tasks() {
        let mut s = state("consent");
        assert!(generation(&s, vec![story()]).is_err());
        let mut env = envelope();
        env.interview = Some(brief());
        env.interview.as_mut().unwrap().goal.clear();
        assert!(validation::validate(&env, &s, &s.effective_user()).is_err());
        mark_ready(&mut s);
        env = envelope();
        env.task_stories = Some(vec![story()]);
        assert!(validation::validate(&env, &s, &s.effective_user()).is_err());
        assert!(!s.repo_root.join(".kool-ade-packet/planning/tasks").exists());
        let nt = validation::validate(&envelope(), &s, &s.effective_user()).unwrap();
        apply::apply(&mut s, &nt).unwrap();
        assert!(
            !s.workflow.ready(s.spec_text.as_deref()),
            "further discussion invalidates old readiness"
        );
        std::fs::remove_dir_all(s.repo_root).unwrap();
    }

    #[test]
    fn structured_generate_request_keeps_the_current_readiness_snapshot() {
        let mut state = state("typed-action");
        mark_ready(&mut state);
        let ready_before = state.workflow.clone();
        let mut env = envelope();
        env.requested_action = Some(crate::harness::RequestedAction {
            action: crate::harness::ApplicationAction::GenerateTasks,
            target_uid: None,
        });

        let normalized = validation::validate(&env, &state, &state.effective_user()).unwrap();
        assert_eq!(normalized.requested_action, env.requested_action);
        assert!(normalized.workflow.is_none());
        assert_eq!(state.workflow, ready_before);
        assert!(state.workflow.ready(state.planning_contract()));
        std::fs::remove_dir_all(state.repo_root).unwrap();
    }

    #[test]
    fn structured_action_cannot_hide_a_planning_change() {
        let state = state("typed-action-mixed");
        let mut env = envelope();
        env.requested_action = Some(crate::harness::RequestedAction {
            action: crate::harness::ApplicationAction::ApproveChange,
            target_uid: None,
        });
        env.updated_specification = Some("# Unreviewed change".into());

        let problems = validation::validate(&env, &state, &state.effective_user()).unwrap_err();
        assert!(
            problems
                .iter()
                .any(|problem| problem.contains("cannot be combined"))
        );
        assert_eq!(state.items.len(), 0);
        std::fs::remove_dir_all(state.repo_root).unwrap();
    }

    fn feature_document(state_tag: &str, root: &std::path::Path, id: &str, title: &str) -> String {
        let dir = root
            .join(".kool-ade-packet/planning/changes")
            .join(format!("{id}-fixture-{state_tag}"));
        std::fs::create_dir_all(&dir).unwrap();
        let body = format!(
            "# {id}: {title}\n\n**Status:** Ready\n\n## Intent\n\nFixture intent.\n\n## Current Behavior\n\nFixture current.\n\n## Desired Behavior\n\nFixture desired.\n\n## Scope\n\nIn: fixture.\n\n## Requirements\n\n- FIXTURE-R1 (MUST). fixture behavior.\n\n## Decisions and Assumptions\n\n- **A1 (fixture):** recorded.\n\n## Acceptance Criteria\n\n1. Observable fixture outcome.\n"
        );
        let path = dir.join("specification.md");
        let body =
            crate::artifacts::product_docs::identity::preserve_feature_identity(&path, id, &body)
                .unwrap();
        let identity = crate::domain::ArtifactIdentity::from_markdown(&body)
            .unwrap()
            .unwrap();
        let body = crate::domain::ChangeMetadata::write_markdown(
            &body,
            &identity,
            crate::domain::ChangeStatus::Ready,
        )
        .unwrap();
        std::fs::write(path, &body).unwrap();
        body
    }

    #[test]
    fn brief_targeting_an_inactive_feature_cannot_generate_that_batch() {
        let mut s = state("gen-target-mismatch");
        let _inactive = feature_document(
            "gen-target-mismatch",
            &s.repo_root,
            "CHG-098",
            "Other feature",
        );
        let active = feature_document(
            "gen-target-mismatch",
            &s.repo_root,
            "CHG-097",
            "Active feature",
        );
        s.active_feature = Some(("CHG-097".into(), active.clone()));
        s.workflow
            .approved_features
            .insert("CHG-097".into(), feature_contract(&active));
        let mut b = brief();
        b.feature_name = "Other feature (CHG-098)".into();
        s.workflow.brief = Some(b);
        s.workflow.reviewed_specification = Some(active);
        let joined = generation(&s, vec![story()]).unwrap_err().join(" | ");
        assert!(
            joined.contains("targets CHG-098") && joined.contains("active feature is CHG-097"),
            "guard must name both sides of the drift: {joined}"
        );
        assert!(!s.repo_root.join(".kool-ade-packet/planning/tasks").exists());
        std::fs::remove_dir_all(s.repo_root).unwrap();
    }

    #[test]
    fn brief_matching_the_active_feature_passes_the_identity_guard() {
        let mut s = state("gen-target-match");
        let active = feature_document(
            "gen-target-match",
            &s.repo_root,
            "CHG-097",
            "Active feature",
        );
        s.active_feature = Some(("CHG-097".into(), active.clone()));
        s.workflow
            .approved_features
            .insert("CHG-097".into(), feature_contract(&active));
        let mut b = brief();
        b.feature_name = "Active feature (CHG-097)".into();
        s.workflow.brief = Some(b);
        s.workflow.reviewed_specification = Some(active);
        if let Err(errors) = generation(&s, vec![story()]) {
            let joined = errors.join(" | ");
            assert!(
                !joined.contains("Brief ") && !joined.contains("feature id"),
                "identity guard must not trip on a matching target: {joined}"
            );
        }
        std::fs::remove_dir_all(s.repo_root).unwrap();
    }

    #[test]
    fn feature_id_scan_handles_legacy_prose_duplicates_and_short_ids() {
        assert_eq!(
            feature_ids_in("Packet MVP \u{2014} git-native desktop specification planner"),
            Vec::<String>::new()
        );
        assert_eq!(
            feature_ids_in("Cards (CHG-002)"),
            vec!["CHG-002".to_string()]
        );
        assert_eq!(feature_ids_in("Add IDs (F10)"), vec!["F10".to_string()]);
        assert_eq!(
            feature_ids_in("A (CHG-002) and B (CHG-002)"),
            vec!["CHG-002".to_string()]
        );
        assert_eq!(feature_ids_in("bad CHG-0 short id"), Vec::<String>::new());
        let multi = vec!["CHG-001".to_string(), "CHG-002".to_string()];
        assert_eq!(
            brief_target_problem(&multi, Some("CHG-001"), &|_| true).as_deref(),
            Some(
                "Brief feature name declares more than one feature id (CHG-001, CHG-002); name exactly one"
            )
        );
        let one = vec!["CHG-099".to_string()];
        assert!(
            brief_target_problem(&one, Some("CHG-001"), &|_| true)
                .unwrap()
                .contains("targets CHG-099")
        );
        assert!(
            brief_target_problem(&one, Some("CHG-001"), &|_| false)
                .unwrap()
                .contains("unknown feature CHG-099")
        );
        assert!(
            brief_target_problem(&one, None, &|_| true)
                .unwrap()
                .contains("no feature is active")
        );
        assert!(brief_target_problem(&[], Some("CHG-001"), &|_| false).is_none());
    }

    #[test]
    fn rejects_incomplete_uncovered_or_misordered_tasks_without_writes() {
        let mut s = state("invalid");
        mark_ready(&mut s);
        let mut task = story();
        task.implementation_steps.clear();
        assert!(generation(&s, vec![task]).is_err());
        let mut task = story();
        task.dependencies = vec![1];
        assert!(generation(&s, vec![task]).is_err());
        let mut task = story();
        task.success_criteria.clear();
        assert!(generation(&s, vec![task]).is_err());
        let mut task = story();
        task.scope_items = vec![2];
        assert!(generation(&s, vec![task]).is_err());
        assert!(!s.repo_root.join(".kool-ade-packet/planning/tasks").exists());
        std::fs::remove_dir_all(s.repo_root).unwrap();
    }

    #[test]
    fn stale_spec_cannot_write_tasks_and_revisions_preserve_previous_batches() {
        let mut s = state("revision");
        mark_ready(&mut s);
        let nt = generation(&s, vec![story()]).unwrap();
        std::fs::write(
            s.repo_root.join(crate::artifacts::SPEC_FILE),
            "# External change",
        )
        .unwrap();
        assert!(apply::apply(&mut s, &nt).is_err());
        assert!(!s.repo_root.join(".kool-ade-packet/planning/tasks").exists());
        assert!(
            !PlannerState::load(&s.repo_root)
                .unwrap()
                .workflow
                .ready(Some("# External change"))
        );
        std::fs::write(
            s.repo_root.join(crate::artifacts::SPEC_FILE),
            s.spec_text.as_ref().unwrap(),
        )
        .unwrap();
        apply::apply(&mut s, &nt).unwrap();
        let first = s.repo_root.join(
            ".kool-ade-packet/planning/tasks/saved-searches/001-persist-named-search-filters.md",
        );
        let original = std::fs::read(&first).unwrap();
        mark_ready(&mut s);
        let mut task = story();
        task.title = "Persist the revised search record".into();
        let nt = generation(&s, vec![task]).unwrap();
        apply::apply(&mut s, &nt).unwrap();
        assert!(
            s.repo_root
                .join(".kool-ade-packet/planning/tasks/saved-searches-02/001-persist-the-revised-search-record.md")
                .exists()
        );
        assert_eq!(std::fs::read(first).unwrap(), original);
        std::fs::remove_dir_all(s.repo_root).unwrap();
    }

    #[test]
    fn batch_publication_rolls_back_on_metadata_failure() {
        let mut s = state("rollback");
        mark_ready(&mut s);
        let nt = generation(&s, vec![story()]).unwrap();
        // Exercise the writer's rollback after publication without mutating a real project.
        std::fs::remove_file(s.repo_root.join(WORKFLOW_FILE)).unwrap();
        std::fs::create_dir(s.repo_root.join(WORKFLOW_FILE)).unwrap();
        let mut workflow = nt.workflow.unwrap();
        assert!(
            crate::artifacts::task_docs::write_batch(
                &s.repo_root,
                &nt.task_batch.unwrap(),
                &mut workflow
            )
            .is_err()
        );
        assert_eq!(
            std::fs::read_dir(s.repo_root.join(".kool-ade-packet/planning/tasks"))
                .unwrap()
                .count(),
            0
        );
        assert!(workflow.task_batches.is_empty());
        std::fs::remove_dir_all(s.repo_root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn generated_paths_cannot_escape_through_symlinks() {
        let mut s = state("symlink");
        mark_ready(&mut s);
        let nt = generation(&s, vec![story()]).unwrap();
        std::fs::create_dir_all(s.repo_root.join(".kool-ade-packet/planning")).unwrap();
        std::os::unix::fs::symlink(
            std::env::temp_dir(),
            s.repo_root.join(".kool-ade-packet/planning/tasks"),
        )
        .unwrap();
        assert!(apply::apply(&mut s, &nt).is_err());
        assert_eq!(
            crate::artifacts::task_docs::slug("../../Bad / Feature"),
            "bad-feature"
        );
        assert!(crate::artifacts::task_docs::slug(&"界".repeat(100)).len() <= 56);
        std::fs::remove_dir_all(s.repo_root).unwrap();
    }
    #[test]
    fn blocking_questions_prevent_a_readiness_offer() {
        let mut s = state("blocking");
        s.items.push(crate::domain::OpenItem::new(
            "CLR-001".into(),
            crate::domain::Priority::Blocking,
            crate::domain::ItemKind::Question,
            "General".into(),
            Some("All".into()),
            "Which user workflow is required?".into(),
            "Scope cannot be established yet.".into(),
        ));
        let mut env = envelope();
        env.interview = Some(brief());
        assert!(
            validation::validate(&env, &s, &s.effective_user())
                .unwrap_err()
                .iter()
                .any(|e| e.contains("blocking"))
        );
        std::fs::remove_dir_all(s.repo_root).unwrap();
    }
}
