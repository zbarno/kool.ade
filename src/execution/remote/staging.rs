//! Durable, bounded file transfer primitives for remote workspace and results.

mod chunk;
mod frame;
mod manifest;
mod receiver;
mod sender;
mod worker;

pub use chunk::{MAX_TRANSFER_CHUNK_BYTES, TransferChunkV1};
pub use frame::{
    MAX_TRANSFER_FRAME_BYTES, TransferFrameV1, decode_transfer_frame, encode_transfer_frame,
    read_transfer_frame, write_transfer_frame,
};
pub use manifest::{TransferManifestV1, TransferPayloadV1};
pub use receiver::{StagingReceiver, VerifiedWorkspaceResult};
pub use sender::stream_file_chunks;
pub use worker::WorkerStager;
