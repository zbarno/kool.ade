use crate::{
    persistence::remote_workers::{self, RemoteWorkerRegistration},
    ui::theme,
};
use std::{
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

mod connection;
mod resource_limit;

const JOB_ID: &str = "execution_environment_remote_worker_job";
const RESULT_ID: &str = "execution_environment_remote_worker_result";
const FORM_ID: &str = "execution_environment_remote_worker_form";

#[derive(Clone, Default)]
struct Form {
    worker_id: String,
    host_alias: String,
    principal: String,
    fingerprint: String,
    consent: bool,
    open: bool,
}

#[derive(Clone)]
struct Job {
    receiver: Arc<Mutex<mpsc::Receiver<Result<String, String>>>>,
}

impl Default for Job {
    fn default() -> Self {
        let (_sender, receiver) = mpsc::channel();
        Self {
            receiver: Arc::new(Mutex::new(receiver)),
        }
    }
}

pub(super) fn paint(ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let pending = poll(&ctx);
    let form_id = egui::Id::new(FORM_ID);
    let mut form = ctx.data_mut(|data| data.get_temp::<Form>(form_id).unwrap_or_default());
    if ui
        .button(if form.open {
            "Cancel add worker"
        } else {
            "Add worker"
        })
        .clicked()
    {
        form.open = !form.open;
        form.consent = false;
    }
    if form.open {
        paint_form(ui, &ctx, &mut form, pending);
    }
    ctx.data_mut(|data| data.insert_temp(form_id, form));
    if let Some(message) = ctx.data_mut(|data| data.get_temp::<String>(egui::Id::new(RESULT_ID))) {
        ui.colored_label(
            if message.starts_with("Passed:") {
                theme::SUCCESS
            } else {
                theme::WARNING
            },
            message,
        );
    }
}

fn paint_form(ui: &mut egui::Ui, ctx: &egui::Context, form: &mut Form, pending: bool) {
    ui.group(|ui| {
        ui.label(egui::RichText::new("Trust a remote worker").strong());
        ui.label(theme::helper_text(
            "Use an SSH alias for a Linux worker running `koolade worker serve --stdio`. The fingerprint must come from a trusted channel.",
        ));
        ui.horizontal(|ui| {
            ui.label("Worker ID");
            ui.text_edit_singleline(&mut form.worker_id);
            ui.label("SSH alias");
            ui.text_edit_singleline(&mut form.host_alias);
            ui.label("SSH user");
            ui.text_edit_singleline(&mut form.principal);
        });
        ui.horizontal(|ui| {
            ui.label("Trusted host-key fingerprint");
            ui.text_edit_singleline(&mut form.fingerprint);
        });
        ui.label(theme::helper_text(
            "The app scans host keys and pins only the key whose SHA256 fingerprint exactly matches this value. It never disables SSH host-key checking.",
        ));
        ui.checkbox(
            &mut form.consent,
            format!(
                "I approve sending task source and authorized dependency snapshots to {}@{}; private file grants remain separate.",
                if form.principal.is_empty() { "<user>" } else { &form.principal },
                if form.host_alias.is_empty() { "<host>" } else { &form.host_alias }
            ),
        );
        let valid = !pending
            && form.consent
            && !form.worker_id.trim().is_empty()
            && !form.host_alias.trim().is_empty()
            && !form.principal.trim().is_empty()
            && !form.fingerprint.trim().is_empty();
        if ui.add_enabled(valid, egui::Button::new("Trust worker")).clicked() {
            let worker_id = form.worker_id.trim().to_owned();
            let host_alias = form.host_alias.trim().to_owned();
            let principal = form.principal.trim().to_owned();
            let fingerprint = form.fingerprint.trim().to_owned();
            start_job(ctx, move || {
                remote_workers::trust(&worker_id, &host_alias, &principal, &fingerprint)
                    .map(|registration| {
                        format!(
                            "Passed: pinned {}@{} as {}. Test its live Docker capabilities before routing work.",
                            registration.ssh_principal,
                            registration.ssh_host_alias,
                            registration.worker_id
                        )
                    })
                    .map_err(|error| error.to_string())
            });
            form.open = false;
        }
    });
}

pub(super) fn paint_worker_actions(ui: &mut egui::Ui, worker: &RemoteWorkerRegistration) {
    let pending = poll(ui.ctx());
    ui.horizontal(|ui| {
        if ui
            .add_enabled(!pending, egui::Button::new("Test connection"))
            .clicked()
        {
            let worker = worker.clone();
            start_job(ui.ctx(), move || connection::test_connection(&worker));
        }
        let revoke_id = egui::Id::new(("remote_worker_revoke", worker.worker_id.as_str()));
        let mut confirm = ui
            .ctx()
            .data_mut(|data| data.get_temp::<bool>(revoke_id).unwrap_or(false));
        if !confirm {
            if ui
                .add_enabled(!pending, egui::Button::new("Revoke trust"))
                .clicked()
            {
                confirm = true;
            }
        } else {
            ui.label("Future sessions to this worker will be blocked.");
            if ui
                .add_enabled(!pending, egui::Button::new("Confirm revoke"))
                .clicked()
            {
                match remote_workers::revoke(&worker.worker_id) {
                    Ok(()) => set_result(
                        ui.ctx(),
                        format!(
                            "Passed: revoked {} and removed its host-key pin.",
                            worker.worker_id
                        ),
                    ),
                    Err(error) => set_result(
                        ui.ctx(),
                        format!("Failed: could not revoke worker: {error}"),
                    ),
                }
                confirm = false;
            }
            if ui.button("Keep trusted").clicked() {
                confirm = false;
            }
        }
        ui.ctx()
            .data_mut(|data| data.insert_temp(revoke_id, confirm));
    });
    if worker
        .last_probe
        .as_ref()
        .is_some_and(|probe| probe.health == remote_workers::RemoteWorkerHealth::Healthy)
        && ui
            .add_enabled(!pending, egui::Button::new("Trust locally approved images"))
            .on_hover_text(
                "Allows only bundled executor digests confirmed by the live worker probe.",
            )
            .clicked()
    {
        let worker = worker.clone();
        start_job(ui.ctx(), move || {
            connection::trust_live_capabilities(&worker)
        });
    }
}

pub(super) fn paint_resource_limit(ui: &mut egui::Ui, worker: &RemoteWorkerRegistration) {
    resource_limit::paint(ui, worker);
}

fn start_job(
    ctx: &egui::Context,
    operation: impl FnOnce() -> Result<String, String> + Send + 'static,
) {
    if ctx
        .data_mut(|data| data.get_temp::<Job>(egui::Id::new(JOB_ID)))
        .is_some()
    {
        return;
    }
    let (sender, receiver) = mpsc::channel();
    if let Err(error) = std::thread::Builder::new()
        .name("koolade-remote-worker-check".into())
        .spawn(move || {
            let _ = sender.send(operation());
        })
    {
        set_result(
            ctx,
            format!("Failed: could not start worker operation: {error}"),
        );
        return;
    }
    ctx.data_mut(|data| {
        data.remove_temp::<String>(egui::Id::new(RESULT_ID));
        data.insert_temp(
            egui::Id::new(JOB_ID),
            Job {
                receiver: Arc::new(Mutex::new(receiver)),
            },
        );
    });
}

fn poll(ctx: &egui::Context) -> bool {
    let Some(job) = ctx.data_mut(|data| data.get_temp::<Job>(egui::Id::new(JOB_ID))) else {
        return false;
    };
    let result = job
        .receiver
        .try_lock()
        .ok()
        .map(|receiver| receiver.try_recv());
    match result {
        Some(Ok(Ok(message))) => finish_job(ctx, message.to_string()),
        Some(Ok(Err(error))) => finish_job(ctx, format!("Failed: {error}")),
        Some(Err(mpsc::TryRecvError::Disconnected)) => {
            finish_job(ctx, "Failed: worker operation ended unexpectedly.".into())
        }
        Some(Err(mpsc::TryRecvError::Empty)) | None => {
            ctx.request_repaint_after(Duration::from_millis(150));
            true
        }
    }
}

fn finish_job(ctx: &egui::Context, message: String) -> bool {
    ctx.data_mut(|data| {
        data.remove_temp::<Job>(egui::Id::new(JOB_ID));
        data.insert_temp(egui::Id::new(RESULT_ID), message);
    });
    false
}

fn set_result(ctx: &egui::Context, message: String) {
    ctx.data_mut(|data| data.insert_temp(egui::Id::new(RESULT_ID), message));
}
