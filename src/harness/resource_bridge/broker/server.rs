use super::super::{MAX_REQUEST_BYTES, ResourceRequest, ResourceResponse, write_json};
use super::BrokerContext;
use super::dependency_flow::prepare_request;
use std::{
    io::{self, Read},
    os::unix::net::UnixStream,
};

pub(super) fn serve(mut client: UnixStream, context: &BrokerContext<'_>) -> io::Result<()> {
    client.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
    client.set_write_timeout(Some(std::time::Duration::from_secs(30)))?;
    let mut request = Vec::new();
    let read = (&mut client)
        .take(MAX_REQUEST_BYTES as u64 + 1)
        .read_to_end(&mut request)?;
    if read == 0 || request.len() > MAX_REQUEST_BYTES {
        return write_json(
            &mut client,
            &ResourceResponse {
                status: "rejected".into(),
                summary: "Resource request is empty or too large.".into(),
                content: None,
                path: None,
                dependency_request: None,
                bytes: 0,
            },
        );
    }
    let parsed =
        serde_json::from_slice::<ResourceRequest>(request.strip_suffix(b"\n").unwrap_or(&request));
    let response = match parsed {
        Ok(request) => match prepare_request(context, &request) {
            Ok(response) => response,
            Err(error) => ResourceResponse {
                status: "error".into(),
                summary: format!("Resource request could not be completed: {error:#}"),
                content: None,
                path: None,
                dependency_request: None,
                bytes: 0,
            },
        },
        Err(error) => ResourceResponse {
            status: "rejected".into(),
            summary: format!("Resource request was invalid: {error}"),
            content: None,
            path: None,
            dependency_request: None,
            bytes: 0,
        },
    };
    if let Some(request) = response.dependency_request.as_ref()
        && let Ok(mut pending) = context.dependency.lock()
    {
        *pending = Some(request.clone());
    }
    if response.status != "allowed"
        && response.status != "prepared"
        && let Ok(mut pending) = context.attention.lock()
        && pending.is_none()
    {
        *pending = Some(response.summary.clone());
    }
    write_json(&mut client, &response)
}
