use super::super::{Dialog, KooladeApp, Screen};
use crate::app::dialogs::DlgCodeRepository;

impl KooladeApp {
    pub(super) fn render_code_repository_dialog(
        &mut self,
        ui: &mut egui::Ui,
        mut draft: DlgCodeRepository,
    ) {
        let register = std::cell::RefCell::new(false);
        let cancel = std::cell::RefCell::new(false);
        let browse = std::cell::RefCell::new(false);
        let viewport = ui.ctx().content_rect();
        let bounds = egui::Rect::from_center_size(
            viewport.center(),
            egui::vec2(viewport.width().min(720.0), viewport.height().min(520.0)),
        );
        let closed =
            crate::ui::overlays::show_panel_modal(ui, "Register code repository", bounds, |ui| {
                let (save, close, choose) = draft.paint(ui);
                *register.borrow_mut() = save;
                *cancel.borrow_mut() = close;
                *browse.borrow_mut() = choose;
            });

        if *browse.borrow() {
            let current = std::path::PathBuf::from(draft.checkout.trim());
            let start = current
                .is_dir()
                .then_some(current)
                .or_else(|| std::env::current_dir().ok());
            let mut picker = rfd::FileDialog::new().set_title("Choose a code repository checkout");
            if let Some(start) = start {
                picker = picker.set_directory(start);
            }
            if let Some(path) = picker.pick_folder() {
                let path = path.canonicalize().unwrap_or(path);
                draft.checkout = path.to_string_lossy().into_owned();
            }
        }

        let mut saved = false;
        if *register.borrow() {
            match self.save_code_repository(&draft) {
                Ok(repository_id) => {
                    saved = true;
                    self.toasts
                        .success(format!("Registered code repository {repository_id}"));
                }
                Err(error) => draft.feedback = Some((false, error)),
            }
        }
        if !closed && !*cancel.borrow() && !saved {
            self.dialog = Some(Dialog::AddCodeRepository(draft));
        }
    }

    pub(in crate::app::root) fn save_code_repository(
        &mut self,
        draft: &DlgCodeRepository,
    ) -> Result<String, String> {
        let Screen::Connected(project) = &mut self.screen else {
            return Err(
                "Connect a managed planning workspace before registering a code repository.".into(),
            );
        };
        let store = project.state.planning_store.clone();
        if store.mode != crate::artifacts::planning_store::StoreMode::ManagedLocal
            || store.project_id.is_nil()
        {
            return Err("Code repositories can be added to managed planning workspaces.".into());
        }
        let registration = crate::core::managed_workspace::CodeRepositoryRegistration {
            id: draft.id.trim().to_owned(),
            role: draft.role.trim().to_owned(),
            remote: (!draft.remote.trim().is_empty()).then(|| draft.remote.trim().to_owned()),
            checkout: std::path::PathBuf::from(draft.checkout.trim()),
        };
        let _writer = crate::core::writer_gate::acquire();
        let workspace = crate::core::managed_workspace::WorkspaceManager::from_state_root()
            .register_code_repository(store.project_id, registration)
            .map_err(|error| format!("code repository registration failed: {error:#}"))?;
        project.state.repositories = workspace.repositories;
        project.state.baseline_planning_revision = store.revision().map_err(|error| {
            format!("repository registered but planning state could not reload: {error}")
        })?;
        project.refresh_git();
        Ok(draft.id.trim().to_owned())
    }
}
