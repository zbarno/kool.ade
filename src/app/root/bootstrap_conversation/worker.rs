use crate::harness::{AiHarness, BootstrapConversationResponseV1, PlanningRequest};

pub(super) fn execute(
    harness: &dyn AiHarness,
    request: &PlanningRequest,
) -> Result<BootstrapConversationResponseV1, String> {
    if request.cancel.load(std::sync::atomic::Ordering::Acquire) {
        return Err("cancelled".into());
    }
    let outcome = harness
        .execute(request)
        .map_err(|_| "provider failed".to_owned())?;
    if request.cancel.load(std::sync::atomic::Ordering::Acquire) {
        return Err("cancelled".into());
    }
    BootstrapConversationResponseV1::parse_json(&outcome.final_text)
        .map_err(|_| "provider returned an invalid setup-assistant response".into())
}

#[cfg(test)]
#[path = "worker_tests.rs"]
mod tests;
