use super::*;
mod lifecycle_pr;
mod lifecycle_spec;
mod lifecycle_ui;
mod planning_board;
mod private_file_grants;
mod runtime_capability;
pub(super) mod specification_status;
mod target_choices;
mod telemetry;

impl KooladeApp {
    pub(super) fn activity_samples_for_surface(&self, key: Option<&str>) -> Vec<(i64, u64)> {
        let Screen::Connected(p) = &self.screen else {
            return Vec::new();
        };
        if key.is_none()
            && let Some(overall) = &p.activity.overall
        {
            return overall.telemetry.samples.clone();
        }
        let item_id = key
            .and_then(|key| {
                p.state
                    .items
                    .iter()
                    .chain(&p.state.resolved_items)
                    .find(|item| item.conversation_key() == key)
                    .map(|item| item.id.as_str())
            })
            .or(key);
        let mut buckets = std::collections::BTreeMap::<i64, u64>::new();
        for (id, progress) in &p.activity.tasks {
            if item_id.is_none_or(|key| key == id) {
                for (bucket, count) in &progress.telemetry.samples {
                    *buckets.entry(*bucket).or_default() += count;
                }
            }
        }
        for (id, progress) in &p.activity.conversations {
            if key.is_none_or(|key| key == id) {
                for (bucket, count) in &progress.telemetry.samples {
                    *buckets.entry(*bucket).or_default() += count;
                }
            }
        }
        buckets.into_iter().collect()
    }
    pub(super) fn task_detail_for_surface(
        &mut self,
        ticket: &str,
    ) -> Option<crate::ui::task_detail::ViewModel> {
        let (
            implementation,
            implementation_active,
            queued_failure,
            failure_disposition,
            messages,
            progress,
            conversation_active,
            conversation_error,
            task_status,
            draft,
            auto_build,
            can_start,
            lifecycle_pr,
        ) = match &self.screen {
            Screen::Connected(project) => (
                project.implementation_states.get(ticket).cloned(),
                project.active_implementations.contains_key(ticket),
                project
                    .queue
                    .blocked
                    .get(ticket)
                    .map(|failure| failure.message.clone()),
                project
                    .queue
                    .blocked
                    .get(ticket)
                    .map(|failure| failure.recovery),
                project
                    .task_chats
                    .messages
                    .get(ticket)
                    .map(|messages| messages.to_vec())
                    .unwrap_or_default(),
                project.activity.tasks.get(ticket).cloned(),
                project.task_turns.contains_key(ticket),
                project.task_chats.error.clone(),
                project
                    .task_documents
                    .iter()
                    .find(|document| document.path == ticket)
                    .and_then(|document| document.task_state.as_ref())
                    .map(|state| state.status),
                project
                    .task_chats
                    .drafts
                    .get(ticket)
                    .cloned()
                    .unwrap_or_default(),
                project.queue.auto_build,
                project.active_implementations.len() < project.queue.max_parallel.clamp(1, 8),
                lifecycle_pr::for_ticket(project, ticket),
            ),
            Screen::Welcome => return None,
        };
        let (lifecycle_pr_work_uid, lifecycle_pr_status, lifecycle_pr_error) = lifecycle_pr;
        let lifecycle_spec_proposal = match &self.screen {
            Screen::Connected(project) => lifecycle_spec::for_ticket(project, ticket),
            Screen::Welcome => None,
        };
        let (lifecycle, lifecycle_error) = match &self.screen {
            Screen::Connected(project) => lifecycle_ui::for_ticket(project, ticket),
            Screen::Welcome => (None, None),
        };
        let runtime_capability = match &self.screen {
            Screen::Connected(project) => runtime_capability::for_ticket(project, ticket),
            Screen::Welcome => None,
        };
        let failure = queued_failure.or_else(|| {
            (!implementation_active)
                .then(|| {
                    implementation
                        .as_ref()
                        .filter(|record| {
                            record.status
                                == crate::core::implementation::ImplementationStatus::Blocked
                        })
                        .map(|record| record.detail.clone())
                })
                .flatten()
        });
        let (implementation_metrics, feature_metrics) = match &self.screen {
            Screen::Connected(project) => telemetry::reports(project, ticket),
            Screen::Welcome => (None, None),
        };
        let target_key = match &self.screen {
            Screen::Connected(project) => super::super::task_target::key(project, ticket),
            Screen::Welcome => ticket.to_owned(),
        };
        let (environment, settings_error) =
            crate::persistence::execution_environment_settings::load();
        let target_preference = environment
            .task_targets
            .get(&target_key)
            .cloned()
            .unwrap_or_else(|| environment.default_target.clone());
        let task_capabilities = environment
            .task_capabilities
            .get(&target_key)
            .copied()
            .unwrap_or_default();
        let task_allowed_targets = environment
            .task_allowed_targets
            .get(&target_key)
            .cloned()
            .unwrap_or_default();
        let target_choices = target_choices::build(&target_preference, settings_error.as_deref());
        let remote_private_file_workers = target_choices
            .iter()
            .filter(|choice| choice.available)
            .filter_map(|choice| match &choice.preference {
                crate::execution::contract::TargetPreference::Worker(worker_id) => {
                    Some(crate::ui::task_detail::RemotePrivateFileGrantWorker {
                        worker_id: worker_id.clone(),
                        destination: choice.label.clone(),
                    })
                }
                _ => None,
            })
            .collect();
        let (
            remote_private_file_candidates,
            remote_private_file_candidates_error,
            remote_private_file_candidates_loading,
        ) = match &self.screen {
            Screen::Connected(project) => {
                private_file_grants::for_task(project, ticket, implementation.as_ref())
            }
            Screen::Welcome => (Vec::new(), None, false),
        };
        let grant_identity = implementation.as_ref().and_then(|implementation| {
            implementation.workspace_uid.as_ref().map(|project_id| {
                (
                    project_id.clone(),
                    implementation
                        .task_uid
                        .clone()
                        .unwrap_or_else(|| match &self.screen {
                            Screen::Connected(project) => {
                                super::super::task_target::key(project, ticket)
                            }
                            Screen::Welcome => ticket.to_owned(),
                        }),
                )
            })
        });
        let (remote_private_file_grants, remote_private_file_grants_error) = match grant_identity {
            Some((project_id, task_id)) => {
                match crate::persistence::remote_private_file_grants::active_for_task(
                    &crate::persistence::state_root(),
                    &project_id,
                    &task_id,
                    crate::persistence::harness_settings::current_unix_ms(),
                ) {
                    Ok(grants) => (
                        grants
                            .into_iter()
                            .map(|grant| crate::ui::task_detail::RemotePrivateFileGrantView {
                                grant_id: grant.grant_id,
                                worker_id: grant.worker_id,
                                files: grant
                                    .files
                                    .into_iter()
                                    .map(|file| (file.path, file.data_class))
                                    .collect(),
                                expires_at_unix_ms: grant.expires_at_unix_ms,
                            })
                            .collect(),
                        None,
                    ),
                    Err(error) => (Vec::new(), Some(error.to_string())),
                }
            }
            None => (
                Vec::new(),
                Some("Task workspace identity is unavailable".into()),
            ),
        };
        let execution_task_id = match &self.screen {
            Screen::Connected(project) => super::super::task_target::key(project, ticket),
            Screen::Welcome => ticket.to_owned(),
        };
        let (execution_evidence, execution_evidence_error) =
            if uuid::Uuid::parse_str(&execution_task_id)
                .is_ok_and(|id| id.hyphenated().to_string() == execution_task_id)
            {
                match crate::persistence::execution_records::latest_for_task(&execution_task_id) {
                    Ok(record) => (record, None),
                    Err(error) => (None, Some(error)),
                }
            } else {
                (None, None)
            };
        let attention = failure
            .as_deref()
            .and_then(|detail| self.attention_view(ticket, detail));
        let base = if implementation_active {
            crate::core::implementation::board_column(implementation.as_ref(), true)
        } else if let Some(status) = task_status {
            status.board_column()
        } else if failure.is_some() {
            3
        } else {
            crate::core::implementation::board_column(implementation.as_ref(), false)
        };
        let board_column = if implementation_active {
            base
        } else {
            crate::ui::task_chat::board_column(base, &messages, conversation_active)
        };
        Some(crate::ui::task_detail::ViewModel {
            implementation,
            implementation_active,
            failure,
            failure_disposition,
            attention,
            messages,
            progress,
            conversation_active,
            conversation_error,
            task_status,
            board_column,
            can_start,
            auto_build,
            draft,
            activity_samples: self.activity_samples(Some(ticket)),
            activity_active: self.activity_active(ticket),
            implementation_metrics,
            feature_metrics,
            target_preference,
            task_capabilities,
            task_allowed_targets,
            target_choices,
            remote_private_file_candidates,
            remote_private_file_candidates_error,
            remote_private_file_candidates_loading,
            remote_private_file_workers,
            remote_private_file_grants,
            remote_private_file_grants_error,
            execution_evidence,
            execution_evidence_error,
            lifecycle_pr_work_uid,
            lifecycle_pr_status,
            lifecycle_pr_error,
            lifecycle_spec_proposal,
            runtime_capability,
            lifecycle,
            lifecycle_error,
        })
    }
}
