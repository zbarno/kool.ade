use std::os::unix::net::UnixStream;

use crate::computer_use::{
    ComputerActionV1,
    guest_protocol::{GuestOperationResultV1, GuestProtocolErrorV1},
};

use super::protocol::{ReplyV1, RequestV1, read_message, write_message};

pub(crate) fn launch(app_id: &str) -> Result<(), GuestProtocolErrorV1> {
    let reply = request(RequestV1::Launch {
        app_id: app_id.to_owned(),
    })?;
    match reply {
        ReplyV1::Completed => Ok(()),
        _ => Err(GuestProtocolErrorV1::UnsupportedCapability),
    }
}

pub(crate) fn manage(
    action: &ComputerActionV1,
) -> Result<GuestOperationResultV1, GuestProtocolErrorV1> {
    let reply = request(RequestV1::Manage {
        action: action.clone(),
    })?;
    Ok(match reply {
        ReplyV1::Completed => GuestOperationResultV1::Completed,
        ReplyV1::Failed => GuestOperationResultV1::Failed,
        ReplyV1::MissingDependencies => GuestOperationResultV1::MissingDependencies,
        _ => GuestOperationResultV1::UnsupportedCapability,
    })
}

pub(crate) struct StagedTaskClientV1 {
    stream: UnixStream,
}

impl StagedTaskClientV1 {
    pub(crate) fn begin(action: &ComputerActionV1) -> Result<Self, GuestProtocolErrorV1> {
        let stream = UnixStream::connect(super::protocol::SOCKET)
            .map_err(|_| GuestProtocolErrorV1::UnsupportedCapability)?;
        let mut client = Self { stream };
        client.send(RequestV1::StageTaskBegin {
            action: action.clone(),
        })?;
        Ok(client)
    }

    pub(crate) fn write_chunk(
        &mut self,
        artifact_index: u16,
        offset: u64,
        bytes: Vec<u8>,
    ) -> Result<(), GuestProtocolErrorV1> {
        self.send(RequestV1::StageTaskChunk {
            artifact_index,
            offset,
            bytes,
        })
    }

    pub(crate) fn finish(mut self) -> Result<(), GuestProtocolErrorV1> {
        self.send(RequestV1::StageTaskFinish)
    }

    fn send(&mut self, message: RequestV1) -> Result<(), GuestProtocolErrorV1> {
        write_message(&mut self.stream, &message)?;
        match read_message(&mut self.stream)? {
            ReplyV1::Completed => Ok(()),
            _ => Err(GuestProtocolErrorV1::UnsupportedCapability),
        }
    }
}

pub(crate) fn launch_staged_task(
    action: &ComputerActionV1,
) -> Result<GuestOperationResultV1, GuestProtocolErrorV1> {
    let reply = request(RequestV1::LaunchStagedTask {
        action: action.clone(),
    })?;
    Ok(match reply {
        ReplyV1::Completed => GuestOperationResultV1::Completed,
        ReplyV1::Failed => GuestOperationResultV1::Failed,
        _ => GuestOperationResultV1::UnsupportedCapability,
    })
}

pub(crate) fn app_management_available() -> bool {
    matches!(
        status(),
        Some(ReplyV1::Status {
            app_management: true,
            ..
        })
    )
}

pub(crate) fn staged_task_app_launch_available() -> bool {
    matches!(
        status(),
        Some(ReplyV1::Status {
            staged_task_app_launch: true,
            ..
        })
    )
}

pub(crate) fn installed_app_ids() -> Vec<String> {
    let Some(ReplyV1::Status { installed_apps, .. }) = status() else {
        return Vec::new();
    };
    if installed_apps.len() > 64
        || installed_apps.iter().any(|app| {
            app.is_empty()
                || app.len() > 64
                || !app.bytes().all(|byte| {
                    byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || byte == b'-'
                        || byte == b'.'
                })
        })
    {
        return Vec::new();
    }
    installed_apps
}

fn status() -> Option<ReplyV1> {
    request(RequestV1::Status).ok()
}

fn request(message: RequestV1) -> Result<ReplyV1, GuestProtocolErrorV1> {
    let mut stream = UnixStream::connect(super::protocol::SOCKET)
        .map_err(|_| GuestProtocolErrorV1::UnsupportedCapability)?;
    write_message(&mut stream, &message)?;
    read_message(&mut stream)
}
