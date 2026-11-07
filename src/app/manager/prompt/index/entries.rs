use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum TaskIndexKind {
    Task,
    HumanQuestion,
    Planning,
    Review,
    QualityAssurance,
    OtherWork,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct TaskIndexEntry {
    pub id: String,
    pub kind: TaskIndexKind,
    pub status: String,
    pub priority: Option<String>,
    pub authority: Option<String>,
    pub updated_revision: Option<u64>,
    pub owner: Option<String>,
    pub dependencies: Vec<String>,
    pub blockers: Vec<String>,
    pub latest_evidence: Option<String>,
    pub detail: String,
    rank: u8,
}

impl TaskIndexEntry {
    pub(super) fn prompt_line(&self) -> String {
        format!(
            "{} {:?} {} priority={:?} authority={:?} rev={:?} owner={:?} dependencies={:?} blockers={:?} evidence={:?}: {}",
            self.id,
            self.kind,
            self.status,
            self.priority,
            self.authority,
            self.updated_revision,
            self.owner,
            self.dependencies,
            self.blockers,
            self.latest_evidence,
            self.detail
        )
    }
}

pub(super) fn build(
    project: &crate::app::session::Project,
    focus_ids: &[String],
) -> Vec<TaskIndexEntry> {
    let mut result = Vec::new();
    let tasks_by_uid = project
        .task_documents
        .iter()
        .filter_map(|document| {
            document
                .metadata
                .as_ref()
                .map(|metadata| (metadata.uid.as_str(), document))
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    for doc in &project.task_documents {
        if doc.path.ends_with("/README.md") || project.archived_tasks.contains(&doc.path) {
            continue;
        }
        let uid = doc
            .metadata
            .as_ref()
            .map(|metadata| metadata.uid.as_str())
            .or_else(|| doc.identity.as_ref().map(|identity| identity.uid.as_str()))
            .unwrap_or(&doc.path);
        let execution = project.implementation_states.get(&doc.path);
        let status = if project.active_implementations.contains_key(&doc.path) {
            "running"
        } else if project.queue.blocked.contains_key(&doc.path) {
            "needs-attention"
        } else {
            execution
                .map(|state| state.status.wire_name())
                .unwrap_or("todo")
        };
        let rank = if status == "needs-attention" {
            0
        } else if status == "running" {
            1
        } else if matches!(
            execution.map(|state| state.status),
            Some(
                crate::core::implementation::ImplementationStatus::Blocked
                    | crate::core::implementation::ImplementationStatus::WaitingToMerge
                    | crate::core::implementation::ImplementationStatus::WaitingForIndependentChecks
                    | crate::core::implementation::ImplementationStatus::Interrupted
            )
        ) {
            2
        } else {
            5
        };
        let mut dependencies = Vec::new();
        let mut blockers = Vec::new();
        for dependency_uid in doc
            .metadata
            .as_ref()
            .into_iter()
            .flat_map(|metadata| metadata.dependency_uids.iter())
        {
            let dependency = tasks_by_uid.get(dependency_uid.as_str()).copied();
            let dependency_path = dependency
                .map(|candidate| candidate.path.clone())
                .unwrap_or_else(|| format!("missing:{dependency_uid}"));
            dependencies.push(dependency_path.clone());
            let complete = dependency.is_some_and(|candidate| {
                project
                    .implementation_states
                    .get(&candidate.path)
                    .is_some_and(|state| {
                        state.status == crate::core::implementation::ImplementationStatus::Completed
                            || state.pr_state
                                == Some(crate::core::implementation::PullRequestState::Merged)
                    })
            });
            if !complete {
                blockers.push(dependency_path);
            }
        }
        if project.queue.blocked.contains_key(&doc.path)
            || execution.is_some_and(|state| {
                state.status == crate::core::implementation::ImplementationStatus::Blocked
            })
        {
            blockers.push(format!("execution:{uid}"));
        }
        let updated_revision = doc.task_state.as_ref().map(|state| state.revision);
        let latest_evidence = if project.activity.tasks.contains_key(&doc.path) {
            Some(format!("task-activity:{uid}"))
        } else if project.queue.blocked.contains_key(&doc.path) {
            Some(format!("queue-failure:{uid}"))
        } else if updated_revision.is_some() || execution.is_some() {
            Some(format!("implementation-state:{uid}"))
        } else {
            None
        };
        result.push(TaskIndexEntry {
            id: format!("task:{uid}"),
            kind: TaskIndexKind::Task,
            status: status.into(),
            priority: None,
            authority: None,
            updated_revision,
            owner: None,
            dependencies,
            blockers,
            latest_evidence,
            detail: crate::core::context_build::clip(&doc.title, 90),
            rank,
        });
    }
    for item in &project.state.items {
        let key = item.uid.as_deref().unwrap_or(&item.id);
        let replied = project
            .task_chats
            .has_pending_user_reply(item.conversation_key());
        let failed = project
            .task_chats
            .has_failed_task_reply(item.conversation_key());
        result.push(TaskIndexEntry {
            id: format!("question:{key}"),
            kind: TaskIndexKind::HumanQuestion,
            status: if failed {
                "reply-processing-failed"
            } else if replied {
                "reply-saved-needs-processing"
            } else {
                "open"
            }
            .into(),
            priority: Some(format!("{:?}", item.priority)),
            updated_revision: Some(item.record_revision),
            authority: Some(format!("{:?}", item.authority)),
            owner: None,
            dependencies: Vec::new(),
            blockers: item.blocked_by.clone(),
            latest_evidence: (!item.evidence.is_empty()).then(|| format!("planning-item:{key}")),
            detail: crate::core::context_build::clip(&item.question, 90),
            rank: match item.priority {
                crate::domain::Priority::Blocking => 2,
                crate::domain::Priority::High => 3,
                crate::domain::Priority::Normal => 4,
            },
        });
    }
    for work in &project.planning_work {
        if work.task_intent_archived_at_ms.is_some() {
            continue;
        }
        let (kind, rank) = match work.kind {
            crate::core::planning_work::WorkKind::PlanReviewDisposition
            | crate::core::planning_work::WorkKind::CodeReviewDisposition => {
                (TaskIndexKind::Review, work_rank(work.status))
            }
            crate::core::planning_work::WorkKind::QaFindingDisposition => {
                (TaskIndexKind::QualityAssurance, work_rank(work.status))
            }
            crate::core::planning_work::WorkKind::Question => {
                (TaskIndexKind::HumanQuestion, work_rank(work.status))
            }
            crate::core::planning_work::WorkKind::ProjectPlanning => {
                (TaskIndexKind::Planning, work_rank(work.status))
            }
            _ => (TaskIndexKind::OtherWork, work_rank(work.status)),
        };
        let mut dependencies = Vec::new();
        if let Some(parent) = &work.parent_uid {
            dependencies.push(format!("parent:{parent}"));
        }
        result.push(TaskIndexEntry {
            id: format!("work:{}", work.uid),
            kind,
            status: format!("{:?}", work.status),
            priority: None,
            authority: None,
            updated_revision: Some(work.record_revision),
            owner: None,
            dependencies,
            blockers: Vec::new(),
            latest_evidence: Some(format!("planning-work:{}", work.uid)),
            detail: crate::core::context_build::clip(&work.title, 90),
            rank,
        });
    }
    result.sort_by(|a, b| {
        let a_focused = focus_matches(&a.id, focus_ids);
        let b_focused = focus_matches(&b.id, focus_ids);
        let bucket = |entry: &TaskIndexEntry, focused| {
            if entry.rank <= 1 {
                entry.rank
            } else if focused {
                2
            } else {
                entry.rank.saturating_add(1)
            }
        };
        bucket(a, a_focused)
            .cmp(&bucket(b, b_focused))
            .then_with(|| a.rank.cmp(&b.rank))
            .then_with(|| a.id.cmp(&b.id))
    });
    result
}

fn focus_matches(id: &str, focus_ids: &[String]) -> bool {
    focus_ids.iter().any(|focus| {
        focus == id
            || contains_exact_id(focus, id)
            || id.strip_prefix("task:").is_some_and(|uid| focus == uid)
    })
}

fn contains_exact_id(text: &str, id: &str) -> bool {
    text.match_indices(id).any(|(start, matched)| {
        let end = start + matched.len();
        let boundary = |character: Option<char>| {
            character.is_none_or(|character| {
                !(character.is_ascii_alphanumeric()
                    || matches!(character, '-' | '_' | ':' | '/' | '.'))
            })
        };
        boundary(text[..start].chars().next_back()) && boundary(text[end..].chars().next())
    })
}

fn work_rank(status: crate::core::planning_work::WorkStatus) -> u8 {
    if status == crate::core::planning_work::WorkStatus::NeedsAttention {
        0
    } else if status == crate::core::planning_work::WorkStatus::InProgress {
        1
    } else {
        6
    }
}

#[cfg(test)]
mod tests {
    use super::contains_exact_id;

    #[test]
    fn focused_identifiers_require_token_boundaries() {
        assert!(contains_exact_id(
            "event for task:abc-1 started",
            "task:abc-1"
        ));
        assert!(!contains_exact_id(
            "event for task:abc-10 started",
            "task:abc-1"
        ));
        assert!(!contains_exact_id(
            "event for task:foobar started",
            "task:foo"
        ));
    }
}
