use std::sync::mpsc::{self, Receiver, TryRecvError};

use crate::app::bootstrap_approval::{ApprovalGate, ApprovalPreview, ApprovalRequest};
use crate::app::bootstrap_repair_policy::Proposal;

enum Completion {
    Preview(Result<(Proposal, ApprovalPreview), String>),
    Prepared {
        gate: ApprovalGate,
        result: Result<(), String>,
    },
}

#[derive(Clone, Copy)]
enum WorkKind {
    Preview,
    Prepare,
}

pub(crate) struct Controller {
    gate: Option<ApprovalGate>,
    proposal: Option<Proposal>,
    preview: Option<ApprovalPreview>,
    receiver: Option<Receiver<Completion>>,
    work_kind: Option<WorkKind>,
    result: Option<Result<(), String>>,
}

impl Default for Controller {
    fn default() -> Self {
        Self {
            gate: Some(ApprovalGate::new()),
            proposal: None,
            preview: None,
            receiver: None,
            work_kind: None,
            result: None,
        }
    }
}

impl Controller {
    pub(crate) fn paint(&mut self, ui: &mut egui::Ui, prerequisite_id: &str) -> bool {
        if !prerequisite_id.starts_with("setup:images:general:") {
            return false;
        }
        let completed = self.poll(ui.ctx());
        ui.add_space(crate::ui::theme::spacing::M);
        ui.label(crate::ui::theme::section_heading(
            "Prepare the approved image",
        ));
        ui.label(
            "This downloads the architecture-matched, digest-locked general runtime image if needed, then runs a bounded non-root smoke test with networking disabled. The operation uses the local Docker Engine and does not access project files, run host shell commands, or request administrator privileges.",
        );

        if let Some(kind) = self.work_kind {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(match kind {
                    WorkKind::Preview => "Checking the local Docker Engine and pinned image…",
                    WorkKind::Prepare => "Preparing the pinned image and checking isolation…",
                });
            });
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        } else if let Some(preview) = &self.preview {
            let request = preview.show_confirmation(ui);
            let cancel = ui.button("Cancel review").clicked();
            if let Some(request) = request {
                self.begin_prepare(request);
            } else if cancel {
                self.preview = None;
                self.proposal = None;
            }
        } else {
            if let Some(result) = &self.result {
                match result {
                    Ok(()) => ui.colored_label(
                        crate::ui::theme::SUCCESS,
                        "Passed: the approved runtime image is ready and its isolation check passed.",
                    ),
                    Err(error) => ui.colored_label(crate::ui::theme::DANGER, error),
                };
            }
            let label = if matches!(self.result.as_ref(), Some(Err(_))) {
                "Review and retry image preparation"
            } else {
                "Review image preparation"
            };
            if ui.button(label).clicked() {
                self.begin_preview(prerequisite_id.to_owned());
            }
        }
        completed
    }

    fn begin_preview(&mut self, prerequisite_id: String) {
        if self.receiver.is_some() {
            return;
        }
        self.result = None;
        self.preview = None;
        self.proposal = None;
        let (sender, receiver) = mpsc::channel();
        self.receiver = Some(receiver);
        self.work_kind = Some(WorkKind::Preview);
        std::thread::spawn(move || {
            let result = super::executor::preview(&prerequisite_id);
            let _ = sender.send(Completion::Preview(result));
        });
    }

    fn begin_prepare(&mut self, request: ApprovalRequest) {
        if self.receiver.is_some() {
            return;
        }
        let Some(mut gate) = self.gate.take() else {
            self.result = Some(Err("An image preparation is already active.".into()));
            return;
        };
        let Some(proposal) = self.proposal.clone() else {
            self.gate = Some(gate);
            self.result = Some(Err(
                "The approved image preview expired; review it again.".into()
            ));
            return;
        };
        if request.proposal() != &proposal {
            self.gate = Some(gate);
            self.result = Some(Err("The image preview changed; review it again.".into()));
            self.preview = None;
            self.proposal = None;
            return;
        }
        self.preview = None;
        self.result = None;
        let (sender, receiver) = mpsc::channel();
        self.receiver = Some(receiver);
        self.work_kind = Some(WorkKind::Prepare);
        std::thread::spawn(move || {
            let result = (|| {
                let runtime = super::executor::runtime()?;
                let ticket = gate.grant(request).map_err(|error| error.to_string())?;
                let action = runtime
                    .block_on(gate.consume(ticket, &proposal))
                    .map_err(|error| error.to_string())?;
                drop(runtime);
                super::executor::prepare_approved(action, &proposal)
            })();
            let _ = sender.send(Completion::Prepared { gate, result });
        });
    }

    pub(crate) fn poll(&mut self, ctx: &egui::Context) -> bool {
        let Some(receiver) = &self.receiver else {
            return false;
        };
        let completion = receiver.try_recv();
        match completion {
            Ok(Completion::Preview(Ok((proposal, preview)))) => {
                self.proposal = Some(proposal);
                self.preview = Some(preview);
                self.receiver = None;
                self.work_kind = None;
                ctx.request_repaint();
                false
            }
            Ok(Completion::Preview(Err(error))) => {
                self.result = Some(Err(error));
                self.receiver = None;
                self.work_kind = None;
                ctx.request_repaint();
                false
            }
            Ok(Completion::Prepared { gate, result }) => {
                self.gate = Some(gate);
                self.result = Some(result);
                self.receiver = None;
                self.work_kind = None;
                crate::execution::docker::environment::DockerEnvironmentStatus::invalidate();
                ctx.request_repaint();
                true
            }
            Err(TryRecvError::Empty) => false,
            Err(TryRecvError::Disconnected) => {
                self.gate.get_or_insert_with(ApprovalGate::new);
                self.result = Some(Err(
                    "Image preparation stopped before it could report a result.".into(),
                ));
                self.receiver = None;
                self.work_kind = None;
                ctx.request_repaint();
                false
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn queue_success_for_test(&mut self) {
        let (sender, receiver) = mpsc::channel();
        self.receiver = Some(receiver);
        self.work_kind = Some(WorkKind::Prepare);
        sender
            .send(Completion::Prepared {
                gate: ApprovalGate::new(),
                result: Ok(()),
            })
            .unwrap();
    }
}
