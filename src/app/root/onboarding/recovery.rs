use super::*;
use crate::core::managed_workspace::{
    ArtifactPresence, RecoveryItem, RecoveryRequest, WorkspaceManager,
};

#[derive(Default)]
pub(in crate::app::root) struct Recovery {
    request: Option<RecoveryRequest>,
    items: Vec<RecoveryItem>,
    failed: bool,
    job: Option<std::thread::JoinHandle<Vec<RecoveryItem>>>,
}

impl Recovery {
    pub(super) fn invalidate(&mut self) {
        *self = Self::default();
    }

    pub(super) fn paint(&mut self, ui: &mut egui::Ui, request: RecoveryRequest) {
        if self.request.as_ref() != Some(&request) {
            self.items.clear();
            self.failed = false;
            self.request = Some(request.clone());
            let manager = WorkspaceManager::from_state_root();
            self.job = Some(std::thread::spawn(move || {
                manager.inspect_setup_recovery(&request)
            }));
        }
        if self.job.as_ref().is_some_and(|job| job.is_finished()) {
            match self.job.take().unwrap().join() {
                Ok(items) => self.items = items,
                Err(_) => self.failed = true,
            }
        }
        ui.collapsing("What is already saved?", |ui| {
            if self.job.is_some() {
                ui.spinner();
                ui.label("Checking this request’s locations…");
            } else if self.failed {
                ui.label("Could not inspect saved work. Your files were preserved; try checking again.");
            } else {
                for item in &self.items {
                    ui.label(format!("{}: {}", item.label, match item.presence {
                        ArtifactPresence::Found => "Found",
                        ArtifactPresence::NotFound => "Not found",
                        ArtifactPresence::Unavailable => "Could not check yet",
                    }));
                }
            }
            if ui.button("Check saved work again").clicked() { self.invalidate(); }
            ui.label("This is a read-only snapshot, not proof that setup is complete. Files are preserved. Approving this same request checks ownership and resumes only its missing work; choosing another location keeps the earlier request available.");
        });
        if self.job.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }
    }
}
