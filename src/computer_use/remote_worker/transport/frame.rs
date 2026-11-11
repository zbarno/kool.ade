use serde::{Serialize, de::DeserializeOwned};
use std::io;

pub const MAX_REMOTE_COMPUTER_WORKER_FRAME_BYTES_V1: usize = 1024 * 1024;

/// Encode one length-prefixed JSON RPC frame. Guest display bytes and artifacts
/// travel only through separately bounded artifact references.
pub fn encode_remote_computer_worker_frame_v1<T: Serialize>(value: &T) -> io::Result<Vec<u8>> {
    let body = serde_json::to_vec(value).map_err(|_| invalid_data("frame JSON is invalid"))?;
    if body.is_empty() || body.len() > MAX_REMOTE_COMPUTER_WORKER_FRAME_BYTES_V1 {
        return Err(invalid_data("frame exceeds the size limit"));
    }
    let length = u32::try_from(body.len()).map_err(|_| invalid_data("frame length is invalid"))?;
    let mut frame = Vec::with_capacity(4 + body.len());
    frame.extend_from_slice(&length.to_be_bytes());
    frame.extend_from_slice(&body);
    Ok(frame)
}

/// Decode exactly one complete length-prefixed frame. Trailing or truncated
/// bytes fail closed, avoiding parser ambiguity on worker reconnects.
pub fn decode_remote_computer_worker_frame_v1<T: DeserializeOwned>(frame: &[u8]) -> io::Result<T> {
    if frame.len() < 4 {
        return Err(invalid_data("frame prefix is truncated"));
    }
    let prefix: [u8; 4] = frame[..4]
        .try_into()
        .map_err(|_| invalid_data("frame prefix is invalid"))?;
    let length = u32::from_be_bytes(prefix) as usize;
    if length == 0
        || length > MAX_REMOTE_COMPUTER_WORKER_FRAME_BYTES_V1
        || frame.len() != length.saturating_add(4)
    {
        return Err(invalid_data("frame length is invalid"));
    }
    serde_json::from_slice(&frame[4..]).map_err(|_| invalid_data("frame JSON is invalid"))
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
#[path = "frame/tests.rs"]
mod tests;
