use std::io::{Read, Write};

use crate::computer_use::{ComputerActionV1, guest_protocol::GuestProtocolErrorV1};
use serde::{Deserialize, Serialize};

pub(super) const SOCKET: &str = "/run/koolade-app-helper/control.sock";
pub(super) const MAX_HELPER_FRAME: usize = 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum RequestV1 {
    Launch {
        app_id: String,
    },
    Manage {
        action: ComputerActionV1,
    },
    StageTaskBegin {
        action: ComputerActionV1,
    },
    StageTaskChunk {
        artifact_index: u16,
        offset: u64,
        bytes: Vec<u8>,
    },
    StageTaskFinish,
    LaunchStagedTask {
        action: ComputerActionV1,
    },
    Status,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum ReplyV1 {
    Completed,
    Unsupported,
    Failed,
    MissingDependencies,
    Status {
        app_management: bool,
        staged_task_app_launch: bool,
        installed_apps: Vec<String>,
    },
}

pub(super) fn write_message<W: Write, T: Serialize>(
    writer: &mut W,
    value: &T,
) -> Result<(), GuestProtocolErrorV1> {
    let bytes = serde_json::to_vec(value).map_err(|_| GuestProtocolErrorV1::Malformed)?;
    if bytes.is_empty() || bytes.len() > MAX_HELPER_FRAME {
        return Err(GuestProtocolErrorV1::Oversized);
    }
    writer
        .write_all(&(bytes.len() as u32).to_le_bytes())
        .map_err(|_| GuestProtocolErrorV1::Io)?;
    writer
        .write_all(&bytes)
        .map_err(|_| GuestProtocolErrorV1::Io)
}

pub(super) fn read_message<R: Read, T: for<'de> Deserialize<'de>>(
    reader: &mut R,
) -> Result<T, GuestProtocolErrorV1> {
    let mut size = [0; 4];
    reader
        .read_exact(&mut size)
        .map_err(|_| GuestProtocolErrorV1::Io)?;
    let size = u32::from_le_bytes(size) as usize;
    if size == 0 || size > MAX_HELPER_FRAME {
        return Err(GuestProtocolErrorV1::Oversized);
    }
    let mut bytes = vec![0; size];
    reader
        .read_exact(&mut bytes)
        .map_err(|_| GuestProtocolErrorV1::Io)?;
    serde_json::from_slice(&bytes).map_err(|_| GuestProtocolErrorV1::Malformed)
}
