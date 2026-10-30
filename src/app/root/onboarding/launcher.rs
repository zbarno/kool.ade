use super::*;
use crate::core::managed_workspace::{LocalWorkspaceSummary, WorkspaceManager};

#[derive(Default)]
pub(in crate::app::root) struct Launcher {
    pub available: Vec<LocalWorkspaceSummary>,
    pub unavailable: Vec<(uuid::Uuid, String, bool)>,
    pub error: Option<String>,
    loaded: bool,
    generation: u64,
    pub history_unavailable: bool,
    job: Option<Lookup>,
}
struct Lookup {
    generation: u64,
    result: std::thread::JoinHandle<Result<Snapshot, String>>,
}
struct Snapshot {
    history_unavailable: bool,
    available: Vec<LocalWorkspaceSummary>,
    unavailable: Vec<(uuid::Uuid, String, bool)>,
}
impl Launcher {
    pub(in crate::app::root) fn invalidate(&mut self) {
        self.loaded = false;
        self.generation = self.generation.saturating_add(1);
    }
    pub(super) fn checking(&self) -> bool {
        self.job.is_some()
    }
    pub(super) fn poll(&mut self, ui: &egui::Ui) {
        if self
            .job
            .as_ref()
            .is_some_and(|job| job.result.is_finished())
        {
            let job = self.job.take().unwrap();
            if job.generation == self.generation {
                match job
                    .result
                    .join()
                    .unwrap_or_else(|_| Err("Workspace lookup stopped.".into()))
                {
                    Ok(snapshot) => {
                        self.available = snapshot.available;
                        self.unavailable = snapshot.unavailable;
                        self.history_unavailable = snapshot.history_unavailable;
                        self.error = None;
                    }
                    Err(error) => self.error = Some(error),
                }
                self.loaded = true;
            }
        }
        if !self.loaded && self.job.is_none() {
            let manager = WorkspaceManager::from_state_root();
            self.job = Some(Lookup {
                generation: self.generation,
                result: std::thread::spawn(move || {
                    let listing = manager
                        .list_launcher()
                        .map_err(|error| format!("{error:#}"))?;
                    let mut available = listing.available;
                    let history_unavailable = manager.sort_by_recent_open(&mut available).is_err();
                    let unavailable = listing
                        .unavailable
                        .into_iter()
                        .map(|id| {
                            let (title, missing) = manager.unavailable_workspace_label(id);
                            (id, title, missing)
                        })
                        .collect();
                    Ok(Snapshot {
                        history_unavailable,
                        available,
                        unavailable,
                    })
                }),
            });
        }
        if self.checking() {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }
    }
}
