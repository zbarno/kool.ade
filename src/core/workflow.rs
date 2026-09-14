//! Interview readiness, explicit task-generation consent, and detailed story validation.
use crate::core::{state::PlannerState, validation::NormalizedTurn};
use crate::harness::TurnEnvelope;
use serde::{Deserialize, Serialize};

pub const WORKFLOW_FILE: &str = ".planner/workflow.json";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TurnPurpose {
    #[default]
    Interview,
    GenerateTasks,
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

pub fn validate_outline(
    brief: &InterviewBrief,
    outline: &[TaskOutline],
) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    let mut titles = std::collections::HashSet::new();
    let mut scope = std::collections::HashSet::new();
    let mut criteria = std::collections::HashSet::new();
    if outline.is_empty() || outline.len() > 200 {
        errors.push("The outline must contain between 1 and 200 tasks.".into());
    }
    for (i, task) in outline.iter().enumerate() {
        let n = i + 1;
        if !descriptive_title(&task.title)
            || !substantive(&task.purpose)
            || !titles.insert(task.title.trim().to_lowercase())
        {
            errors.push(format!(
                "Outline task {n}: supply a unique title and concrete purpose."
            ));
        }
        if task.dependencies.iter().any(|d| *d == 0 || *d >= n) {
            errors.push(format!(
                "Outline task {n}: dependencies must refer to earlier tasks."
            ));
        }
        if task.scope_items.is_empty() {
            errors.push(format!(
                "Outline task {n}: at least one scope reference is required."
            ));
        }
        for r in &task.scope_items {
            if *r == 0 || *r > brief.in_scope.len() {
                errors.push(format!("Outline task {n}: invalid scope reference."));
            } else {
                scope.insert(*r);
            }
        }
        for r in &task.success_criteria {
            if *r == 0 || *r > brief.success_criteria.len() {
                errors.push(format!("Outline task {n}: invalid success criterion."));
            } else {
                criteria.insert(*r);
            }
        }
    }
    if scope.len() != brief.in_scope.len() || criteria.len() != brief.success_criteria.len() {
        errors.push("The outline must cover every scope item and success criterion.".into());
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
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
    let path = crate::artifacts::product_docs::document_path(repo, &format!("feature:{id}"))?;
    let text = std::fs::read_to_string(path)?;
    crate::core::specification::validate_feature(id, &text)?;
    anyhow::ensure!(
        text.contains("**Status:** Ready") || text.contains("**Status:** Implementing"),
        "Only a ready or already implementing feature may be approved"
    );
    let contract = feature_contract(&text);
    anyhow::ensure!(!contract.trim().is_empty(), "Feature contract is empty");
    workflow.approved_features.insert(id.to_string(), contract);
    crate::artifacts::task_docs::save_workflow(repo, workflow)?;
    crate::core::gitops::commit(
        repo,
        &format!("planner: approve feature {id}"),
        &[WORKFLOW_FILE.to_string()],
    )
    .map_err(|e| anyhow::anyhow!("approval saved but checkpoint failed: {e}"))
}

pub fn feature_approved(repo: &std::path::Path, workflow: &Workflow, id: &str) -> bool {
    let Ok(path) = crate::artifacts::product_docs::document_path(repo, &format!("feature:{id}"))
    else {
        return false;
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
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

pub fn confirms_generation(text: &str) -> bool {
    matches!(
        text.trim()
            .trim_end_matches(['.', '!'])
            .to_ascii_lowercase()
            .as_str(),
        "yes"
            | "yes please"
            | "proceed"
            | "go ahead"
            | "generate tasks"
            | "generate task stories"
            | "yes, please"
            | "yes, generate tasks"
            | "yes generate tasks"
            | "yes, proceed"
    )
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
        if let Some(id) = &feature_id {
            if !feature_approved(&state.repo_root, &workflow, id) {
                return Err(vec![format!("{id} needs explicit implementation approval")]);
            }
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
    } else {
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
                let spec = updated_feature
                    .or(nt.spec_markdown.as_deref())
                    .or(state.planning_contract())
                    .unwrap_or_default();
                if spec.trim().is_empty() {
                    return Err(vec![
                        "A specification is required before offering task generation.".into(),
                    ]);
                }
                workflow.reviewed_specification = Some(spec.to_owned());
                nt.next_question_id = None;
                nt.assistant_message.push_str(&format!("\n\nThe goal and scope for {} are ready to break down. Would you like to proceed to task generation? Choose Generate task stories, reply yes, or keep refining the plan.", brief.feature_name));
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

/// Titles should name a concrete operation and its object, not just a phase.
pub fn descriptive_title(title: &str) -> bool {
    let words: Vec<_> = title.split_whitespace().collect();
    words.len() >= 4
        && title.chars().count() <= 140
        && !title.contains(['\n', '\r'])
        && !matches!(
            title.trim().to_ascii_lowercase().as_str(),
            "implement the core feature"
                | "set up the project"
                | "add tests and documentation"
                | "implement the remaining functionality"
        )
        && substantive(title)
}

/// A structural floor for implementable stories, not a substitute for semantic review.
/// Applied to EACH response so incomplete stories are repaired before proceeding.
pub fn story_detail_errors(t: &TaskStory, n: usize) -> Vec<String> {
    let mut errors = Vec::new();
    if !descriptive_title(&t.title) {
        errors.push(format!("Task {n}: use a descriptive 4+ word title naming the action, component and behavior (maximum 140 characters)."));
    }
    for (name, text, minimum) in [
        ("intent", &t.intent, 15),
        ("goal", &t.goal, 12),
        ("context", &t.context, 45),
        ("user story", &t.user_story, 12),
        ("purpose", &t.purpose, 8),
        ("rollout notes", &t.rollout_notes, 20),
    ] {
        if !substantive(text) || text.split_whitespace().count() < minimum {
            errors.push(format!("Task {n}: {name} needs at least {minimum} words of task-specific context, behavior or constraints."));
        }
    }
    for (name, values, count, words) in [
        ("affected files", &t.affected_files, 1, 6),
        ("technical design", &t.technical_design, 3, 12),
        ("implementation steps", &t.implementation_steps, 5, 12),
        ("acceptance criteria", &t.acceptance_criteria, 4, 12),
        ("test plan", &t.test_plan, 4, 12),
        ("edge cases", &t.edge_cases, 3, 12),
        ("verification commands", &t.verification_commands, 1, 6),
        ("definition of done", &t.definition_of_done, 3, 8),
    ] {
        let unique: std::collections::HashSet<_> =
            values.iter().map(|v| v.trim().to_lowercase()).collect();
        if values.len() < count
            || unique.len() != values.len()
            || values
                .iter()
                .any(|v| !substantive(v) || v.split_whitespace().count() < words)
        {
            errors.push(format!("Task {n}: {name} requires {count}+ distinct concrete entries, each with {words}+ words; describe exact actions, inputs and expected outcomes."));
        }
    }
    let total = [
        &t.intent,
        &t.goal,
        &t.context,
        &t.user_story,
        &t.purpose,
        &t.rollout_notes,
    ]
    .iter()
    .map(|s| s.split_whitespace().count())
    .sum::<usize>()
        + [
            &t.affected_files,
            &t.technical_design,
            &t.implementation_steps,
            &t.acceptance_criteria,
            &t.test_plan,
            &t.edge_cases,
            &t.verification_commands,
            &t.definition_of_done,
        ]
        .iter()
        .flat_map(|v| v.iter())
        .map(|s| s.split_whitespace().count())
        .sum::<usize>();
    if total < 450 {
        errors.push(format!("Task {n}: only {total} words of implementation detail; provide at least 450 task-specific words without padding or repeated boilerplate."));
    }
    errors
}

fn validate_stories(b: &InterviewBrief, stories: &[TaskStory]) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    if stories.is_empty() || stories.len() > 200 {
        errors.push("Return between 1 and 200 complete task stories.".into());
    }
    let mut titles = std::collections::HashSet::new();
    let mut scope = std::collections::HashSet::new();
    let mut criteria = std::collections::HashSet::new();
    for (i, t) in stories.iter().enumerate() {
        let n = i + 1;
        if !titles.insert(t.title.trim().to_lowercase()) {
            errors.push(format!("Task {n}: duplicate title."));
        }
        errors.extend(story_detail_errors(t, n));
        if t.dependencies.iter().any(|d| *d == 0 || *d >= n) {
            errors.push(format!("Task {n}: dependencies must reference earlier tasks, preventing missing references and cycles."));
        }
        if t.scope_items.is_empty() {
            errors.push(format!(
                "Task {n}: reference at least one approved scope item."
            ));
        }
        for r in &t.scope_items {
            if *r == 0 || *r > b.in_scope.len() {
                errors.push(format!("Task {n}: invalid scope reference {r}."));
            } else {
                scope.insert(*r);
            }
        }
        for r in &t.success_criteria {
            if *r == 0 || *r > b.success_criteria.len() {
                errors.push(format!("Task {n}: invalid success criterion {r}."));
            } else {
                criteria.insert(*r);
            }
        }
    }
    if scope.len() != b.in_scope.len() || criteria.len() != b.success_criteria.len() {
        errors.push(
            "Task stories must cover every approved scope item and success criterion.".into(),
        );
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
        let mut envelope: TurnEnvelope =
            serde_json::from_str(include_str!("../../tests/fixtures/task-story-1.json")).unwrap();
        let mut story = envelope.task_stories.take().unwrap().remove(0);
        story.success_criteria = vec![1];
        story
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
        env.updated_specification = Some(crate::core::specification::fixture(
            "Resume named filters across sessions. Persist and restore filters without changing ad hoc searches.",
        ));
        let nt = validation::validate(&env, state, &state.effective_user()).unwrap();
        assert!(
            nt.assistant_message
                .contains("Would you like to proceed to task generation?")
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
            !s.repo_root.join("planning/tasks").exists(),
            "readiness must not generate files"
        );
        let mut second = story();
        second.title = "Connect the saved search picker".into();
        second.dependencies = vec![1];
        let nt = generation(&s, vec![story(), second]).unwrap();
        let receipt = apply::apply(&mut s, &nt).unwrap();
        let dir = s.repo_root.join("planning/tasks/saved-searches");
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
        assert!(!s.repo_root.join("planning/tasks").exists());
        let nt = validation::validate(&envelope(), &s, &s.effective_user()).unwrap();
        apply::apply(&mut s, &nt).unwrap();
        assert!(
            !s.workflow.ready(s.spec_text.as_deref()),
            "further discussion invalidates old readiness"
        );
        assert!(confirms_generation("Yes, please!"));
        assert!(!confirms_generation("yes but change the scope first"));
        assert!(!confirms_generation("not yet"));
        std::fs::remove_dir_all(s.repo_root).unwrap();
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
        assert!(!s.repo_root.join("planning/tasks").exists());
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
        assert!(!s.repo_root.join("planning/tasks").exists());
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
        let first = s
            .repo_root
            .join("planning/tasks/saved-searches/001-persist-named-search-filters.md");
        let original = std::fs::read(&first).unwrap();
        mark_ready(&mut s);
        let mut task = story();
        task.title = "Persist the revised search record".into();
        let nt = generation(&s, vec![task]).unwrap();
        apply::apply(&mut s, &nt).unwrap();
        assert!(
            s.repo_root
                .join("planning/tasks/saved-searches-02/001-persist-the-revised-search-record.md")
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
            std::fs::read_dir(s.repo_root.join("planning/tasks"))
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
        std::os::unix::fs::symlink(std::env::temp_dir(), s.repo_root.join("planning/tasks"))
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
