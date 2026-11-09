use std::path::{Path, PathBuf};
use std::sync::{Arc, atomic::AtomicBool, mpsc::Sender};

use crate::{
    computer_use::mcp::{ComputerUseMcpRequestV1, ComputerUseMcpResponseV1},
    harness::{ComputerUseMcpAccessV1, LiveProgress},
};

#[derive(Clone, Debug)]
pub(crate) struct ComputerUseBridgeBindingV1 {
    pub socket_path: PathBuf,
    pub bridge_session_id: String,
    pub capability_id: String,
    pub task_id: String,
}

pub(crate) struct ComputerUseBridge {
    pub binding: ComputerUseBridgeBindingV1,
}

impl ComputerUseBridge {
    pub(crate) fn start(
        _: &Path,
        _: Arc<ComputerUseMcpAccessV1>,
        _: Arc<AtomicBool>,
        _: Sender<LiveProgress>,
    ) -> anyhow::Result<Self> {
        anyhow::bail!("Computer Use MCP is not supported on this host")
    }
}

pub(crate) fn tool_names() -> Vec<String> {
    Vec::new()
}

pub(crate) fn call(
    _: &Path,
    _: &ComputerUseBridgeBindingV1,
    _: ComputerUseMcpRequestV1,
) -> anyhow::Result<ComputerUseMcpResponseV1> {
    anyhow::bail!("Computer Use MCP is not supported on this host")
}
