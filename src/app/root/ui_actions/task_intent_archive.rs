use super::super::{KooladeApp, Screen};

#[cfg(test)]
#[path = "task_intent_archive/tests.rs"]
mod tests;

impl KooladeApp {
    pub(super) fn archive_task(&mut self, ticket: &str) {
        let done = matches!(&self.screen, Screen::Connected(project)
            if crate::core::planning_work::cards(&project.state, &project.planning_work)
                .iter().any(|work| work.key == ticket && work.status == crate::core::planning_work::WorkStatus::Done))
            || match &self.screen {
                Screen::Connected(project) => {
                    project
                        .state
                        .resolved_items
                        .iter()
                        .any(|item| item.conversation_key() == ticket)
                        || project
                            .implementation_states
                            .get(ticket)
                            .is_some_and(|state| {
                                state.status == super::super::ImplementationStatus::Completed
                            })
                }
                _ => false,
            };
        let task_intent_index = match &self.screen {
            Screen::Connected(project) => project.planning_work.iter().position(|work| {
                work.key == ticket
                    && work.parent_uid.is_none()
                    && work.kind.starts_task_intent()
                    && work.task_intent_root_uid() == Some(work.uid.as_str())
            }),
            Screen::Welcome => None,
        };
        if let Some(index) = task_intent_index {
            if !done {
                return;
            }
            let result = if let Screen::Connected(project) = &mut self.screen {
                let Some(root) = project.planning_work.get(index) else {
                    return;
                };
                let board = task_intent_board(project);
                if !board.can_archive_task_intent(root) {
                    self.toasts.warning(
                        "TaskIntent cannot be archived until its current cycle reaches a verified terminal outcome.",
                    );
                    return;
                }
                let Some(root) = project.planning_work.get_mut(index) else {
                    return;
                };
                if root.task_intent_archived_at_ms.is_some() {
                    return;
                }
                root.task_intent_archived_at_ms = Some(now_ms());
                match project.save_planning_work() {
                    Ok(()) => Ok(()),
                    Err(error) => {
                        if let Some(root) = project.planning_work.get_mut(index) {
                            root.task_intent_archived_at_ms = None;
                        }
                        Err(error)
                    }
                }
            } else {
                return;
            };
            if let Err(error) = result {
                self.toasts
                    .danger(format!("Could not archive TaskIntent: {error}"));
            }
            return;
        }
        let result = {
            let Screen::Connected(project) = &mut self.screen else {
                return;
            };
            if project.active_implementations.contains_key(ticket)
                || project.task_turns.contains_key(ticket)
                || !done
            {
                return;
            }
            let mut archived = project.archived_tasks.clone();
            archived.insert(ticket.to_string());
            crate::persistence::archived_tasks::save(&project.chat_slug, &archived)
                .inspect(|()| project.archived_tasks = archived)
        };
        if let Err(error) = result {
            self.toasts
                .danger(format!("Could not archive task: {error}"));
        }
    }

    pub(super) fn restore_task_intent(&mut self, root_uid: &str) {
        let result = if let Screen::Connected(project) = &mut self.screen {
            let Some(index) = project.planning_work.iter().position(|work| {
                work.uid == root_uid
                    && work.parent_uid.is_none()
                    && work.kind.starts_task_intent()
                    && work.task_intent_root_uid() == Some(root_uid)
            }) else {
                self.toasts
                    .warning("TaskIntent root is no longer available.");
                return;
            };
            let key = project.planning_work[index].key.clone();
            let previous = project.planning_work[index].task_intent_archived_at_ms;
            let mut archived = project.archived_tasks.clone();
            let had_legacy_archive = archived.remove(&key);
            if had_legacy_archive
                && let Err(error) =
                    crate::persistence::archived_tasks::save(&project.chat_slug, &archived)
            {
                Err(error.to_string())
            } else {
                project.planning_work[index].task_intent_archived_at_ms = None;
                match project.save_planning_work() {
                    Ok(()) => {
                        project.archived_tasks = archived;
                        Ok(())
                    }
                    Err(error) => {
                        project.planning_work[index].task_intent_archived_at_ms = previous;
                        let rollback = if had_legacy_archive {
                            crate::persistence::archived_tasks::save(
                                &project.chat_slug,
                                &project.archived_tasks,
                            )
                        } else {
                            Ok(())
                        };
                        Err(rollback.map_or_else(
                            |rollback| format!("{error}; archive rollback failed: {rollback}"),
                            |_| error.to_string(),
                        ))
                    }
                }
            }
        } else {
            Err("Connect a project before restoring a TaskIntent.".into())
        };
        match result {
            Ok(()) => self
                .toasts
                .success("TaskIntent restored to the planning board."),
            Err(error) => self
                .toasts
                .danger(format!("Could not restore TaskIntent: {error}")),
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1, |elapsed| {
            elapsed.as_millis().max(1).min(u128::from(u64::MAX)) as u64
        })
}

fn task_intent_board(
    project: &crate::app::session::Project,
) -> crate::ui::planning_board::ViewModel {
    let lifecycle = crate::core::lifecycle::LifecycleStore::new(&project.state.planning_store);
    let lifecycle_features = lifecycle.list_features().unwrap_or_default();
    let lifecycle_feature_tasks = lifecycle.list_tasks().unwrap_or_default();
    let lifecycle_tasks = project
        .task_documents
        .iter()
        .filter_map(|document| {
            let uid = document.identity.as_ref()?.uid.as_str();
            lifecycle_feature_tasks
                .iter()
                .find(|task| task.task_uid == uid)
                .map(|task| (document.path.clone(), task.phase))
        })
        .collect();
    let lifecycle_deployment_evidence = lifecycle_features
        .iter()
        .filter_map(|feature| {
            let current = lifecycle
                .load_current_implementation(&feature.feature_uid)
                .ok()??;
            let deployment = lifecycle
                .load_deployment_evidence(&feature.feature_uid)
                .ok()??;
            Some((feature.feature_uid.clone(), (deployment, current)))
        })
        .collect();
    let planning_items = project
        .state
        .items
        .iter()
        .chain(&project.state.resolved_items)
        .cloned()
        .collect();
    let active_work_keys = project
        .active_implementations
        .keys()
        .chain(project.task_turns.keys())
        .chain(project.queue.in_flight.iter())
        .chain(project.queue.waiting_for_capacity.iter())
        .chain(project.queue.current_ticket.iter())
        .cloned()
        .collect();
    crate::ui::planning_board::ViewModel {
        planning_work: project.planning_work.clone(),
        planning_items,
        task_documents: project.task_documents.clone(),
        cancelled: project.cancelled_work.clone(),
        lifecycle_tasks,
        lifecycle_features,
        lifecycle_feature_tasks,
        lifecycle_deployment_evidence,
        active_work_keys,
        ..Default::default()
    }
}
