use std::sync::{Arc, Mutex, mpsc};

#[derive(Clone)]
struct SmokeTestJob {
    receiver: Arc<Mutex<mpsc::Receiver<Result<(), String>>>>,
}

impl Default for SmokeTestJob {
    fn default() -> Self {
        let (_sender, receiver) = mpsc::channel();
        Self {
            receiver: Arc::new(Mutex::new(receiver)),
        }
    }
}

pub(super) fn start(ctx: &egui::Context) -> Result<(), String> {
    let job_id = egui::Id::new("execution_environment_smoke_job");
    if ctx
        .data_mut(|data| data.get_temp::<SmokeTestJob>(job_id))
        .is_some()
    {
        return Ok(());
    }
    let (sender, receiver) = mpsc::channel();
    std::thread::Builder::new()
        .name("koolade-docker-smoke-test".into())
        .spawn(move || {
            let result = crate::execution::docker::environment::run_smoke_test()
                .map_err(|failure| failure.message);
            let _ = sender.send(result);
        })
        .map_err(|error| format!("Could not start the Docker smoke test: {error}"))?;
    ctx.data_mut(|data| {
        data.remove_temp::<String>(egui::Id::new("execution_environment_smoke_result"));
        data.insert_temp(
            job_id,
            SmokeTestJob {
                receiver: Arc::new(Mutex::new(receiver)),
            },
        );
    });
    Ok(())
}

pub(super) fn poll(ctx: &egui::Context) -> (bool, Option<String>, bool) {
    let job_id = egui::Id::new("execution_environment_smoke_job");
    let job = ctx.data_mut(|data| data.get_temp::<SmokeTestJob>(job_id));
    let Some(job) = job else {
        let result = ctx.data_mut(|data| {
            data.get_temp::<String>(egui::Id::new("execution_environment_smoke_result"))
        });
        return (false, result, false);
    };
    let received = job
        .receiver
        .try_lock()
        .ok()
        .map(|receiver| receiver.try_recv());
    match received {
        Some(Ok(Ok(()))) => finish_smoke_test(
            ctx,
            "Passed: approved smoke container exited cleanly and was removed.",
        ),
        Some(Ok(Err(error))) => finish_smoke_test(ctx, &format!("Failed: {error}")),
        Some(Err(mpsc::TryRecvError::Disconnected)) => finish_smoke_test(
            ctx,
            "Failed: smoke test worker stopped before reporting a result.",
        ),
        Some(Err(mpsc::TryRecvError::Empty)) | None => (true, None, false),
    }
}

fn finish_smoke_test(ctx: &egui::Context, result: &str) -> (bool, Option<String>, bool) {
    ctx.data_mut(|data| {
        data.remove_temp::<SmokeTestJob>(egui::Id::new("execution_environment_smoke_job"));
        data.insert_temp(
            egui::Id::new("execution_environment_smoke_result"),
            result.to_owned(),
        );
    });
    (false, Some(result.to_owned()), true)
}
