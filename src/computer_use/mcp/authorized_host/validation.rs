use super::{Request, Response, Status};

pub(super) fn target_machine_id(session: &crate::computer_use::ComputerSessionV1) -> &str {
    match &session.target {
        crate::computer_use::ComputerTargetV1::LocalKvm { machine_id } => machine_id,
    }
}

pub(super) fn session_id(request: &Request) -> Option<&str> {
    match request {
        Request::StartSession { session_id }
        | Request::Observe { session_id, .. }
        | Request::Action { session_id, .. }
        | Request::Pause { session_id }
        | Request::Resume { session_id }
        | Request::Release { session_id } => Some(session_id),
        Request::Capabilities | Request::RequestSession { .. } => None,
    }
}

pub(super) fn denied(reason: &str) -> Response {
    Response {
        schema_version: super::super::COMPUTER_USE_MCP_VERSION_V1,
        status: Status::Denied,
        summary: "Computer Use request was denied by app-owned authority.".into(),
        reason_code: Some(reason.into()),
        session_id: None,
        capabilities: Vec::new(),
        observation: None,
        action_outcome: None,
    }
}

pub(super) fn blocked(reason: &str) -> Response {
    Response {
        schema_version: super::super::COMPUTER_USE_MCP_VERSION_V1,
        status: Status::Blocked,
        summary: "Computer Use requires an application action before access can continue.".into(),
        reason_code: Some(reason.into()),
        session_id: None,
        capabilities: Vec::new(),
        observation: None,
        action_outcome: None,
    }
}

pub(super) fn resource_budget_within(
    actual: &crate::computer_use::ComputerResourceBudgetV1,
    approved: &crate::computer_use::ComputerResourceBudgetV1,
) -> bool {
    actual.cpu_millis <= approved.cpu_millis
        && actual.memory_bytes <= approved.memory_bytes
        && actual.disk_bytes <= approved.disk_bytes
        && actual.time_limit_ms <= approved.time_limit_ms
        && actual.action_limit <= approved.action_limit
}

pub(super) fn unix_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| {
            duration.as_millis().min(u64::MAX as u128) as u64
        })
}
