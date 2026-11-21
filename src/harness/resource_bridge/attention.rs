use crate::error::AppError;
use crate::harness::{DependencyRequestStatus, LiveProgress};

/// Report broker decisions before accepting a provider's final response.
pub(crate) fn resource_attention(
    bridge: Option<&super::ResourceBridge>,
    progress: &std::sync::mpsc::Sender<LiveProgress>,
) -> Option<AppError> {
    let bridge = bridge?;
    if let Err(detail) = bridge.close_admission() {
        return Some(AppError::Other(format!("Needs Attention: {detail}.")));
    }
    if let Some(request) = bridge.dependency_request() {
        let detail = request.summary();
        let _ = progress.send(LiveProgress {
            activity: Some("Dependency request needs authorization".into()),
            dependency_requests: vec![request.clone()],
            ..Default::default()
        });
        if request.status != DependencyRequestStatus::Prepared {
            return Some(AppError::Other(format!(
                "Needs Attention: a specific dependency request needs review. {detail}"
            )));
        }
    }
    bridge.attention_detail().map(|detail| AppError::Other(format!(
        "Needs Attention: the worker requested a resource that requires operator review. {detail}"
    )))
}
