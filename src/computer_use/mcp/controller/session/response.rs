use super::super::super::{ComputerUseMcpResponseV1, ComputerUseMcpStatusV1};

pub(crate) fn response(
    status: ComputerUseMcpStatusV1,
    summary: &str,
    reason: Option<&str>,
    session_id: Option<String>,
    capabilities: Vec<String>,
) -> ComputerUseMcpResponseV1 {
    ComputerUseMcpResponseV1 {
        schema_version: super::super::super::COMPUTER_USE_MCP_VERSION_V1,
        status,
        summary: summary.into(),
        reason_code: reason.map(str::to_owned),
        session_id,
        capabilities,
        observation: None,
        action_outcome: None,
    }
}

pub(crate) fn denied(reason: &str) -> ComputerUseMcpResponseV1 {
    response(
        ComputerUseMcpStatusV1::Denied,
        "Computer Use request was denied.",
        Some(reason),
        None,
        Vec::new(),
    )
}

pub(crate) fn failed(reason: &str) -> ComputerUseMcpResponseV1 {
    response(
        ComputerUseMcpStatusV1::Failed,
        "Computer Use operation failed safely.",
        Some(reason),
        None,
        Vec::new(),
    )
}

pub(crate) fn completed(session_id: &str) -> ComputerUseMcpResponseV1 {
    response(
        ComputerUseMcpStatusV1::Completed,
        "Computer Use operation completed.",
        None,
        Some(session_id.to_owned()),
        Vec::new(),
    )
}

pub(crate) fn unix_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| {
            duration.as_millis().min(u64::MAX as u128) as u64
        })
}
