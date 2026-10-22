use super::ExecutionSession;
use crate::execution::backend::ExecutionHandleV1;

#[cfg(test)]
pub(crate) fn telemetry_test_session() -> ExecutionSession {
    super::tests::telemetry_test_session()
}

#[derive(Clone, Debug)]
pub(super) struct RemoteTelemetryAttribution {
    pub(super) harness: String,
    pub(super) requested_model: Option<String>,
}

impl ExecutionSession {
    pub(crate) fn set_telemetry_attribution(&self, harness: &str, requested_model: Option<&str>) {
        if !matches!(
            harness,
            "codex" | "claude" | "antigravity" | "opencode" | "copilot" | "pi"
        ) {
            return;
        }
        let requested_model = requested_model.and_then(safe_telemetry_model);
        if let Ok(mut current) = self.remote_telemetry_attribution.lock() {
            match current.as_mut() {
                Some(current) if current.harness == harness => {
                    if current.requested_model.is_none() {
                        current.requested_model = requested_model;
                    }
                }
                Some(_) => {}
                None => {
                    *current = Some(RemoteTelemetryAttribution {
                        harness: harness.to_owned(),
                        requested_model,
                    });
                }
            }
        }
    }

    pub(crate) fn set_telemetry_model(&self, requested_model: Option<&str>) {
        let Some(requested_model) = requested_model.and_then(safe_telemetry_model) else {
            return;
        };
        if let Ok(mut current) = self.remote_telemetry_attribution.lock()
            && let Some(current) = current.as_mut()
            && current.requested_model.is_none()
        {
            current.requested_model = Some(requested_model);
        }
    }

    pub(super) fn copy_telemetry_attribution_to(&self, destination: &ExecutionSession) {
        let attribution = self
            .remote_telemetry_attribution
            .lock()
            .ok()
            .and_then(|value| value.clone());
        if let Some(attribution) = attribution {
            destination.set_telemetry_attribution(
                &attribution.harness,
                attribution.requested_model.as_deref(),
            );
        }
    }

    pub(super) fn record_telemetry_attribution(&self, handle: &ExecutionHandleV1) {
        let crate::execution::contract::TargetLocation::Remote { worker_id } =
            &handle.target.location
        else {
            return;
        };
        let attribution = self
            .remote_telemetry_attribution
            .lock()
            .ok()
            .and_then(|value| value.clone());
        if let Some(attribution) = attribution {
            let _ = crate::persistence::remote_execution_telemetry::record_attribution(
                &handle.execution_id,
                &handle.task_id,
                worker_id,
                &attribution.harness,
                attribution.requested_model.as_deref(),
            );
        }
    }

    #[cfg(test)]
    pub(crate) fn telemetry_attribution_for_test(&self) -> Option<(String, Option<String>)> {
        self.remote_telemetry_attribution
            .lock()
            .ok()?
            .as_ref()
            .map(|value| (value.harness.clone(), value.requested_model.clone()))
    }
}

fn safe_telemetry_model(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:/-@+".contains(&byte)))
    .then(|| value.to_owned())
}
