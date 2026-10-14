use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

mod checks;
use checks::*;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskGraphV1 {
    pub feature_uid: String,
    /// Immutable user-confirmed plan snapshot record.
    pub approved_spec_ref: String,
    /// Exact prior user decisions that constrain this plan.
    pub decision_refs: Vec<String>,
    pub approved_plan_hash: String,
    /// Complete requirement set from the approved plan, used to reject lost scope.
    pub approved_requirement_ids: Vec<String>,
    pub tasks: Vec<TaskDraftV1>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskDraftV1 {
    pub uid: String,
    pub title: String,
    pub goal: String,
    pub intent: String,
    pub non_goals: Vec<String>,
    pub requirements: Vec<String>,
    pub acceptance_criteria: Vec<String>,
    pub tests: Vec<String>,
    pub relevant_files: Vec<String>,
    pub dependencies: Vec<String>,
    pub repository_id: String,
    pub source_branch: String,
    #[serde(default)]
    pub source_sha: Option<String>,
    pub destination_branch: String,
    pub estimated_concurrency_risks: Vec<String>,
    pub subtasks: Vec<SubtaskDraftV1>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubtaskDraftV1 {
    pub uid: String,
    pub parent_uid: String,
    pub title: String,
    pub goal: String,
    pub acceptance_criteria: Vec<String>,
    pub tests: Vec<String>,
    pub depends_on: Vec<String>,
    pub parallelizable: bool,
    #[serde(default)]
    pub affected_paths: Vec<String>,
}

impl TaskGraphV1 {
    pub fn validate(&self) -> Result<(), String> {
        if !canonical_uuid(&self.feature_uid) {
            return Err("task graph must bind a canonical feature UUID".into());
        }
        if !canonical_uuid(&self.approved_spec_ref)
            || self.decision_refs.len() > 128
            || self.decision_refs.iter().any(|uid| !canonical_uuid(uid))
            || self.decision_refs.iter().collect::<HashSet<_>>().len() != self.decision_refs.len()
        {
            return Err(
                "task graph must bind a plan snapshot and unique decision receipt IDs".into(),
            );
        }
        if self.approved_plan_hash.len() != 64
            || !self
                .approved_plan_hash
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("task graph must bind a 64-character approved plan hash".into());
        }
        if self.tasks.is_empty() || self.tasks.len() > 128 {
            return Err("task graph must contain between 1 and 128 standalone tasks".into());
        }
        if self.approved_requirement_ids.is_empty()
            || self.approved_requirement_ids.len() > 512
            || !unique_texts(&self.approved_requirement_ids, 256)
        {
            return Err("task graph must include a bounded unique approved requirement set".into());
        }

        let approved = self
            .approved_requirement_ids
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        let mut indexes = HashMap::new();
        let mut all_uids = HashSet::new();
        for (index, task) in self.tasks.iter().enumerate() {
            if !canonical_uuid(&task.uid)
                || !all_uids.insert(task.uid.as_str())
                || indexes.insert(task.uid.as_str(), index).is_some()
            {
                return Err(
                    "task graph contains an invalid or duplicate standalone task UUID".into(),
                );
            }
            if !bounded_text(&task.title, 256)
                || !bounded_text(&task.goal, 4_000)
                || !bounded_text(&task.intent, 4_000)
                || !bounded_text_list(&task.non_goals, 64, 2_000)
                || !bounded_text_list(&task.requirements, 128, 256)
                || task
                    .requirements
                    .iter()
                    .any(|requirement| !approved.contains(requirement.as_str()))
                || !bounded_text_list(&task.acceptance_criteria, 64, 2_000)
                || !bounded_text_list(&task.tests, 64, 2_000)
                || task.relevant_files.is_empty()
                || !bounded_paths(&task.relevant_files, 256)
                || !bounded_text_list(&task.estimated_concurrency_risks, 32, 1_000)
                || task.estimated_concurrency_risks.is_empty()
                || !valid_repository_id(&task.repository_id)
                || !valid_git_branch(&task.source_branch)
                || !valid_git_branch(&task.destination_branch)
                || task
                    .source_sha
                    .as_deref()
                    .is_some_and(|sha| !valid_git_sha(sha))
                || task.subtasks.len() > 256
            {
                return Err(format!(
                    "standalone task {} is incomplete, out of scope, or exceeds its limits",
                    task.uid
                ));
            }
            if task.requirements.is_empty() {
                return Err(format!(
                    "standalone task {} does not own an approved requirement",
                    task.uid
                ));
            }
        }

        let mut ownership_counts = HashMap::<&str, usize>::new();
        for requirement in self.tasks.iter().flat_map(|task| &task.requirements) {
            *ownership_counts.entry(requirement.as_str()).or_default() += 1;
        }
        if approved
            .iter()
            .any(|requirement| ownership_counts.get(requirement) != Some(&1))
        {
            return Err("each approved requirement must have exactly one standalone owner".into());
        }

        let mut task_edges = vec![Vec::new(); self.tasks.len()];
        let mut task_indegree = vec![0_usize; self.tasks.len()];
        for (index, task) in self.tasks.iter().enumerate() {
            let mut unique = HashSet::new();
            for dependency in &task.dependencies {
                let Some(&dependency_index) = indexes.get(dependency.as_str()) else {
                    return Err(format!(
                        "task {} depends on an unknown standalone task",
                        task.uid
                    ));
                };
                if dependency_index == index || !unique.insert(dependency_index) {
                    return Err(format!(
                        "task {} has a self or duplicate dependency",
                        task.uid
                    ));
                }
                task_edges[dependency_index].push(index);
                task_indegree[index] += 1;
            }
            let mut child_uids = HashSet::new();
            for child in &task.subtasks {
                if !canonical_uuid(&child.uid)
                    || !all_uids.insert(child.uid.as_str())
                    || !child_uids.insert(child.uid.as_str())
                    || child.parent_uid != task.uid
                    || !bounded_text(&child.title, 256)
                    || !bounded_text(&child.goal, 4_000)
                    || !bounded_text_list(&child.acceptance_criteria, 64, 2_000)
                    || !bounded_text_list(&child.tests, 64, 2_000)
                    || !bounded_paths(&child.affected_paths, 256)
                {
                    return Err(format!(
                        "subtask of {} has invalid identity, ownership, or required fields",
                        task.uid
                    ));
                }
            }
        }
        if cyclic(&task_edges, &mut task_indegree) {
            return Err("standalone task dependencies must form an acyclic graph".into());
        }

        for task in &self.tasks {
            let child_indexes = task
                .subtasks
                .iter()
                .enumerate()
                .map(|(index, child)| (child.uid.as_str(), index))
                .collect::<HashMap<_, _>>();
            let mut edges = vec![Vec::new(); task.subtasks.len()];
            let mut indegree = vec![0_usize; task.subtasks.len()];
            for (index, child) in task.subtasks.iter().enumerate() {
                let mut unique = HashSet::new();
                for dependency in &child.depends_on {
                    let Some(&dependency_index) = child_indexes.get(dependency.as_str()) else {
                        return Err(format!(
                            "subtask {} depends on a missing or differently-owned subtask",
                            child.uid
                        ));
                    };
                    if dependency_index == index || !unique.insert(dependency_index) {
                        return Err(format!(
                            "subtask {} has a self or duplicate dependency",
                            child.uid
                        ));
                    }
                    edges[dependency_index].push(index);
                    indegree[index] += 1;
                }
            }
            if cyclic(&edges, &mut indegree) {
                return Err(format!(
                    "subtasks of {} must form an acyclic dependency graph",
                    task.uid
                ));
            }
            reject_parallel_path_conflicts(task)?;
        }
        reject_unordered_task_path_conflicts(self, &indexes, &task_edges)?;
        reject_contradictory_sources(&self.tasks)?;
        Ok(())
    }
}
