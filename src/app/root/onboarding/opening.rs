use super::*;

pub(in crate::app::root) struct OpenJob {
    id: uuid::Uuid,
    result: std::thread::JoinHandle<Result<welcome::PreparedConnection, crate::error::AppError>>,
    interview: Option<(String, String)>,
}

impl KooladeApp {
    pub(super) fn begin_onboarding_open(
        &mut self,
        id: uuid::Uuid,
        interview: Option<(String, String)>,
    ) {
        self.begin_onboarding_open_with_policy(id, interview, false);
    }

    pub(super) fn begin_existing_onboarding_open(&mut self, id: uuid::Uuid) {
        self.begin_onboarding_open_with_policy(id, None, true);
    }

    fn begin_onboarding_open_with_policy(
        &mut self,
        id: uuid::Uuid,
        interview: Option<(String, String)>,
        continue_without_saved_setup: bool,
    ) {
        if self.onboarding_open.is_some() {
            return;
        }
        self.conn_error = None;
        self.onboarding.opening_workspace_id = Some(id);
        if let Some((_, key)) = &interview {
            self.onboarding.pending_interview = Some(welcome::onboarding::InterviewRef {
                workspace_id: id,
                work_key: key.clone(),
            });
        }
        self.onboarding.save();
        let may_continue_unsaved = continue_without_saved_setup
            && self.onboarding.step == welcome::onboarding::Step::ExistingProject
            && self
                .onboarding
                .existing
                .as_ref()
                .is_some_and(|request| !request.create_workspace);
        if self.onboarding.error.is_some() && !may_continue_unsaved {
            self.conn_error = Some("Setup progress could not be saved. Restore access to application storage before opening this request.".into());
            return;
        }
        self.onboarding_open = Some(OpenJob {
            id,
            interview,
            result: std::thread::spawn(move || welcome::prepare_open_workspace(id)),
        });
    }

    pub(super) fn poll_onboarding_open(&mut self) {
        if !self
            .onboarding_open
            .as_ref()
            .is_some_and(|job| job.result.is_finished())
        {
            return;
        }
        let job = self.onboarding_open.take().unwrap();
        if self.onboarding.opening_workspace_id != Some(job.id) {
            return;
        }
        self.onboarding.opening_workspace_id = None;
        match job.result.join() {
            Ok(result) => self.finish_connect(result.map(welcome::PreparedConnection::finish)),
            Err(_) => {
                self.conn_error = Some(
                    "Workspace loading stopped. Your files were preserved; try opening it again."
                        .into(),
                )
            }
        }
        if matches!(self.screen, Screen::Connected(_)) {
            self.onboarding_launcher.invalidate();
            if job.interview.is_some() {
                self.continue_onboarding_interview();
            }
            self.onboarding.go(Step::Projects);
        } else {
            self.onboarding.save();
        }
    }
    fn continue_onboarding_interview(&mut self) {
        let Some(reference) = self.onboarding.pending_interview.clone() else {
            return;
        };
        let Screen::Connected(project) = &self.screen else {
            return;
        };
        if project.state.planning_store.project_id != reference.workspace_id
            || project.active_turn.is_some()
        {
            return;
        }
        let Some(work) = project.planning_work.iter().find(|work| {
            work.key == reference.work_key
                && work.kind == crate::core::planning_work::WorkKind::NewProject
        }) else {
            return;
        };
        let intent = work.request.clone();
        self.start_turn_for_work(
            &intent,
            crate::core::workflow::TurnPurpose::Interview,
            None,
            Some(reference.work_key),
        );
    }

    pub(in crate::app::root) fn paint_onboarding_handoff(&mut self, ui: &mut egui::Ui) {
        let Some(reference) = &self.onboarding.pending_interview else {
            return;
        };
        let Screen::Connected(project) = &self.screen else {
            return;
        };
        if project.state.planning_store.project_id != reference.workspace_id {
            return;
        }
        let work_exists = project.planning_work.iter().any(|work| {
            work.key == reference.work_key
                && work.kind == crate::core::planning_work::WorkKind::NewProject
        });
        let ready = project.active_turn.is_none() && work_exists;
        let mut continue_idea = false;
        let mut dismiss = false;
        egui::Panel::top("onboarding_idea_handoff").show(ui, |ui| {
            if work_exists {
                ui.label("Your original idea is saved with its planning work. You can retry the conversation if your assistant was unavailable.");
            } else {
                ui.label("The saved planning work is no longer available. Use the project workspace to start a new conversation.");
            }
            continue_idea = ui
                .add_enabled(
                    ready,
                    presentation::primary_button("Continue discussing your idea"),
                )
                .clicked();
            dismiss = ui.button("I'll continue from the project workspace").clicked();
        });
        if dismiss {
            self.onboarding.pending_interview = None;
            self.onboarding.save();
        }
        if continue_idea {
            self.continue_onboarding_interview();
        }
    }
}

#[cfg(test)]
mod tests;
