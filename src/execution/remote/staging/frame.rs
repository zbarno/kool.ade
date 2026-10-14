//! Standalone bounded transfer frames for transport adapters.

use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};

use super::{TransferChunkV1, TransferManifestV1};

pub const MAX_TRANSFER_FRAME_BYTES: usize = 768 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum TransferFrameV1 {
    Manifest(TransferManifestV1),
    Chunk(TransferChunkV1),
    Finalize {
        transfer_id: String,
    },
    Committed {
        transfer_id: String,
        payload_sha256: String,
    },
}

impl TransferFrameV1 {
    pub fn validate(&self) -> anyhow::Result<()> {
        match self {
            Self::Manifest(manifest) => manifest.validate(),
            Self::Chunk(chunk) => chunk.validate(),
            Self::Finalize { transfer_id } => validate_transfer_id(transfer_id),
            Self::Committed {
                transfer_id,
                payload_sha256,
            } => {
                validate_transfer_id(transfer_id)?;
                anyhow::ensure!(
                    payload_sha256.len() == 64
                        && payload_sha256.bytes().all(|byte| byte.is_ascii_hexdigit()),
                    "committed transfer digest is invalid"
                );
                Ok(())
            }
        }
    }
}

pub fn encode_transfer_frame(frame: &TransferFrameV1) -> anyhow::Result<Vec<u8>> {
    frame.validate()?;
    let payload = serde_json::to_vec(frame)?;
    anyhow::ensure!(
        !payload.is_empty() && payload.len() <= MAX_TRANSFER_FRAME_BYTES,
        "transfer frame exceeds its size limit"
    );
    let length = u32::try_from(payload.len())?;
    let mut bytes = Vec::with_capacity(payload.len() + 4);
    bytes.extend_from_slice(&length.to_be_bytes());
    bytes.extend_from_slice(&payload);
    Ok(bytes)
}

pub fn decode_transfer_frame(bytes: &[u8]) -> anyhow::Result<TransferFrameV1> {
    anyhow::ensure!(bytes.len() >= 4, "transfer frame prefix is truncated");
    let length = u32::from_be_bytes(bytes[..4].try_into()?) as usize;
    anyhow::ensure!(
        length > 0 && length <= MAX_TRANSFER_FRAME_BYTES && bytes.len() == length + 4,
        "transfer frame length is invalid"
    );
    let frame: TransferFrameV1 = serde_json::from_slice(&bytes[4..])?;
    frame.validate()?;
    Ok(frame)
}

pub fn write_transfer_frame(mut output: impl Write, frame: &TransferFrameV1) -> io::Result<()> {
    let bytes = encode_transfer_frame(frame).map_err(invalid_data)?;
    output.write_all(&bytes)?;
    output.flush()
}

pub fn read_transfer_frame(input: &mut impl Read) -> io::Result<Option<TransferFrameV1>> {
    let mut prefix = [0_u8; 4];
    let mut read = 0;
    while read < prefix.len() {
        match input.read(&mut prefix[read..])? {
            0 if read == 0 => return Ok(None),
            0 => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "transfer frame prefix is truncated",
                ));
            }
            count => read += count,
        }
    }
    let length = u32::from_be_bytes(prefix) as usize;
    if length == 0 || length > MAX_TRANSFER_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "transfer frame exceeds its size limit",
        ));
    }
    let mut bytes = vec![0; length + 4];
    bytes[..4].copy_from_slice(&prefix);
    input.read_exact(&mut bytes[4..])?;
    decode_transfer_frame(&bytes)
        .map(Some)
        .map_err(invalid_data)
}

fn validate_transfer_id(value: &str) -> anyhow::Result<()> {
    let id = uuid::Uuid::parse_str(value)?;
    anyhow::ensure!(
        id.hyphenated().to_string() == value,
        "transfer ID is invalid"
    );
    Ok(())
}

fn invalid_data(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn transfer_frames_round_trip_and_reject_truncation_or_trailing_bytes() {
        let frame = TransferFrameV1::Finalize {
            transfer_id: "00000000-0000-4000-8000-000000000001".into(),
        };
        let encoded = encode_transfer_frame(&frame).unwrap();
        assert_eq!(decode_transfer_frame(&encoded).unwrap(), frame);
        assert!(decode_transfer_frame(&encoded[..encoded.len() - 1]).is_err());
        let mut with_trailing = encoded;
        with_trailing.push(0);
        assert!(decode_transfer_frame(&with_trailing).is_err());
        assert_eq!(read_transfer_frame(&mut Cursor::new([])).unwrap(), None);
    }

    #[test]
    fn transfer_frame_reader_rejects_oversized_prefix_before_allocation() {
        let bytes = ((MAX_TRANSFER_FRAME_BYTES as u32) + 1).to_be_bytes();
        assert_eq!(
            read_transfer_frame(&mut Cursor::new(bytes))
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }
}
