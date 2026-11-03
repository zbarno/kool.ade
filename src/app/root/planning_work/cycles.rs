use super::super::{KooladeApp, Screen};

impl KooladeApp {
    pub(in crate::app::root) fn start_task_intent_follow_up(&mut self, root_uid: &str, goal: &str) {
        let mut failure = None;
        let mut cleanup_warning = None;
        let key = if let Screen::Connected(project) = &mut self.screen {
            let previous = project.planning_work.clone();
            match crate::core::planning_work::start_follow_up_cycle(
                &mut project.planning_work,
                root_uid,
                goal,
            ) {
                Ok(cycle_uid) => {
                    let key = project
                        .planning_work
                        .iter()
                        .find(|work| {
                            work.uid != root_uid
                                && work.task_intent_uid.as_deref() == Some(root_uid)
                                && work.task_intent_cycle_uid.as_deref() == Some(cycle_uid.as_str())
                        })
                        .map(|work| work.key.clone());
                    if let Err(error) = project.save_planning_work() {
                        project.planning_work = previous;
                        failure = Some(format!("Cannot start the follow-up cycle: {error}"));
                        None
                    } else {
                        let root_key = project
                            .planning_work
                            .iter()
                            .find(|work| work.uid == root_uid)
                            .map(|work| work.key.clone());
                        if let Some(root_key) = root_key {
                            let mut archived = project.archived_tasks.clone();
                            if archived.remove(&root_key)
                                && let Err(error) = crate::persistence::archived_tasks::save(
                                    &project.chat_slug,
                                    &archived,
                                )
                            {
                                cleanup_warning = Some(format!(
                                    "The follow-up cycle was saved, but the old archive marker could not be cleared: {error}"
                                ));
                            } else {
                                project.archived_tasks = archived;
                            }
                        }
                        key
                    }
                }
                Err(error) => {
                    failure = Some(error.to_string());
                    None
                }
            }
        } else {
            failure = Some("Connect a project before starting a follow-up cycle.".into());
            None
        };
        if let Some(error) = failure {
            self.toasts.warning(error);
            return;
        }
        let Some(key) = key else {
            self.toasts
                .warning("The follow-up cycle was saved without a workspace.");
            return;
        };
        if let Screen::Connected(project) = &mut self.screen {
            project
                .task_chats
                .drafts
                .insert(key.clone(), goal.trim().into());
        }
        self.submit_task_reply(&key);
        if let Some(warning) = cleanup_warning {
            self.toasts.warning(warning);
        } else if matches!(&self.screen, Screen::Connected(project) if project.task_turns.contains_key(&key))
        {
            self.toasts
                .info("Follow-up cycle started; planning is underway.");
        } else {
            self.toasts
                .warning("Follow-up cycle saved. Open its workspace to continue planning.");
        }
    }
}
