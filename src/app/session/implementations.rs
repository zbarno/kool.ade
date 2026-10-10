use super::Project;

impl Project {
    pub fn refresh_implementations(&mut self) {
        let latest = crate::core::implementation::load_board_states_with_store(
            &self.state.repo_root,
            &self.state.planning_store,
        );
        self.adopt_implementations(latest);
        for ticket in self.implementation_states.keys() {
            if !self.activity.tasks.contains_key(ticket)
                && let Some(activity) =
                    crate::core::implementation::load_activity(&self.state.repo_root, ticket)
            {
                self.activity.tasks.insert(ticket.clone(), activity);
            }
        }
        let finished_ms = chrono::Utc::now().timestamp_millis();
        let tickets = self
            .implementation_states
            .iter()
            .map(|(ticket, state)| (ticket.clone(), state.task_uid.clone(), state.status))
            .collect::<Vec<_>>();
        let mut finalized = Vec::new();
        for (ticket, task_uid, status) in tickets {
            if self.active_implementations.contains_key(&ticket) {
                continue;
            }
            let Some(activity) = self.activity.tasks.get_mut(&ticket) else {
                continue;
            };
            if crate::core::implementation::finalize_terminal_activity_if_stale(
                &self.state.repo_root,
                &ticket,
                task_uid.as_deref(),
                status,
                activity,
                finished_ms,
            ) {
                finalized.push(ticket);
            }
        }
        for ticket in finalized {
            self.activity.mark_ticket_dirty(&ticket);
            self.save_task_activity(&ticket);
        }
    }

    pub fn adopt_implementations(
        &mut self,
        latest: std::collections::BTreeMap<String, crate::core::implementation::Implementation>,
    ) {
        let mut changed_statuses = changed_status_baselines(&self.implementation_states, &latest);
        for (ticket, state) in initial_task_status_baselines(&self.task_documents, &latest) {
            changed_statuses.entry(ticket).or_insert(state);
        }
        for (ticket, state) in &latest {
            if let Some(previous) = self.implementation_states.get(ticket)
                && (previous.status != state.status || previous.pr_state != state.pr_state)
            {
                self.activity.pending.push(format!(
                    "{ticket}: {} → {}; PR {:?}",
                    previous.status, state.status, state.pr_state
                ));
            }
        }
        self.implementation_states = latest;
        self.sync_task_status_records(&changed_statuses);
    }

    pub fn save_task_activity(&mut self, ticket: &str) {
        if let Some(activity) = self.activity.tasks.get(ticket)
            && let Err(error) =
                crate::core::implementation::save_activity(&self.state.repo_root, ticket, activity)
        {
            const MARK: &str = "Activity could not be saved: ";
            let previous = self
                .activity
                .tasks
                .get(ticket)
                .and_then(|progress| progress.activity.clone())
                .unwrap_or_default();
            let head = previous
                .split(MARK)
                .next()
                .unwrap_or("")
                .trim_end_matches('\n');
            let rendered = if head.is_empty() {
                format!("{MARK}{error}")
            } else {
                format!("{head}\n{MARK}{error}")
            };
            self.activity.tasks.get_mut(ticket).unwrap().activity = Some(rendered);
        }
    }

    pub fn refresh_git(&mut self) {
        self.git = crate::core::gitops::snapshot(&self.state.repo_root);
    }
}

fn initial_task_status_baselines(
    documents: &[crate::artifacts::task_docs::TaskDocument],
    latest: &std::collections::BTreeMap<String, crate::core::implementation::Implementation>,
) -> std::collections::BTreeMap<String, crate::core::implementation::Implementation> {
    latest
        .iter()
        .filter_map(|(ticket, state)| {
            documents
                .iter()
                .find(|document| document.path == *ticket)
                .filter(|document| {
                    document.task_state.as_ref().is_some_and(|task_state| {
                        task_state.status == crate::core::planning_work::WorkStatus::Todo
                            && task_state.execution_status.is_none()
                            && task_state.revision == 1
                    })
                })
                .map(|_| (ticket.clone(), state.clone()))
        })
        .collect()
}

fn changed_status_baselines(
    previous: &std::collections::BTreeMap<String, crate::core::implementation::Implementation>,
    latest: &std::collections::BTreeMap<String, crate::core::implementation::Implementation>,
) -> std::collections::BTreeMap<String, crate::core::implementation::Implementation> {
    latest
        .iter()
        .filter_map(|(ticket, state)| {
            previous.get(ticket).and_then(|before| {
                (before.status != state.status
                    || before.pr_state != state.pr_state
                    || before.task_uid != state.task_uid)
                    .then(|| (ticket.clone(), before.clone()))
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{changed_status_baselines, initial_task_status_baselines};
    use crate::core::implementation::Implementation;
    use std::collections::BTreeMap;

    #[test]
    fn status_refresh_does_not_republish_an_unchanged_local_snapshot() {
        let state = Implementation {
            status: crate::core::implementation::ImplementationStatus::Implementing,
            ..test_implementation("task.md")
        };
        let previous = BTreeMap::from([("task.md".to_owned(), state.clone())]);
        let latest = BTreeMap::from([("task.md".to_owned(), state)]);

        assert!(changed_status_baselines(&previous, &latest).is_empty());
    }

    #[test]
    fn changed_local_transition_keeps_its_previous_status_as_a_write_fence() {
        let before = test_implementation("task.md");
        let after = Implementation {
            status: crate::core::implementation::ImplementationStatus::Implementing,
            ..before.clone()
        };
        let previous = BTreeMap::from([("task.md".to_owned(), before.clone())]);
        let latest = BTreeMap::from([("task.md".to_owned(), after)]);

        assert_eq!(
            changed_status_baselines(&previous, &latest).get("task.md"),
            Some(&before)
        );
    }

    #[test]
    fn initial_local_status_only_seeds_a_fresh_todo_record() {
        let document = crate::artifacts::task_docs::TaskDocument {
            path: "task.md".into(),
            title: "Task".into(),
            text: String::new(),
            identity: None,
            metadata: None,
            task_state: Some(crate::artifacts::task_docs::TaskState {
                batch_uid: uuid::Uuid::new_v4().to_string(),
                repository_id: "root".into(),
                dependency_uids: Vec::new(),
                status: crate::core::planning_work::WorkStatus::Todo,
                execution_status: None,
                revision: 1,
            }),
            metadata_error: None,
        };
        let latest = BTreeMap::from([("task.md".into(), test_implementation("task.md"))]);

        assert_eq!(
            initial_task_status_baselines(std::slice::from_ref(&document), &latest).len(),
            1
        );
        let mut explicitly_reset = document;
        explicitly_reset.task_state.as_mut().unwrap().revision = 3;
        assert!(initial_task_status_baselines(&[explicitly_reset], &latest).is_empty());
    }

    fn test_implementation(ticket: &str) -> Implementation {
        Implementation {
            ticket: ticket.to_owned(),
            task_uid: None,
            ticket_text: String::new(),
            approved_specification: None,
            approved_product_context: None,
            completed_dependency_context: None,
            branch: "koolade/test".into(),
            source_branch: None,
            destination_branch: None,
            source_ref: None,
            source_commit: None,
            repository_id: None,
            project_id: None,
            repository_identity: None,
            push_repository: None,
            repository_cache: None,
            task_repository_allocation_key: None,
            base: "main".into(),
            base_commit: "test".into(),
            task_repository: std::env::temp_dir(),
            task_repository_kind: Default::default(),
            task_repository_ready: false,
            task_repositories: Vec::new(),
            task_repository_commits: BTreeMap::new(),
            status: crate::core::implementation::ImplementationStatus::Preparing,
            detail: String::new(),
            pr_url: None,
            verified_head: None,
            auto_merge: false,
            merged_commit: None,
            pr_state: None,
            pr_checked_at: None,
            pr_check_attempted_at: None,
            pr_check_error: None,
            independent_check: None,
            cleanup: Default::default(),
        }
    }
}
