use std::{
    pin::Pin,
    sync::{Arc, Mutex},
};

use bollard::container::LogOutput;
use futures_util::{Stream, StreamExt};

use super::super::runtime::RuntimeExecution;
use crate::execution::{contract::ExecutionEventKindV1, errors::ExecutionError};

use super::state::{disconnected, output_limit_error};

pub(super) async fn collect_output(
    mut output: Pin<Box<dyn Stream<Item = Result<LogOutput, bollard::errors::Error>> + Send>>,
    execution: Arc<Mutex<RuntimeExecution>>,
    budget: u64,
) -> Result<(), ExecutionError> {
    while let Some(item) = output.next().await {
        let output = item.map_err(|_| disconnected())?;
        let (stderr, bytes) = match output {
            LogOutput::StdOut { message } => (false, message),
            LogOutput::StdErr { message } => (true, message),
            _ => continue,
        };
        let mut offset = 0;
        while offset < bytes.len() {
            let mut state = execution.lock().map_err(|_| super::state::disconnected())?;
            let remaining = budget.saturating_sub(state.output_bytes) as usize;
            if remaining == 0 {
                return Err(output_limit_error());
            }
            let end = (offset + 16 * 1024)
                .min(bytes.len())
                .min(offset + remaining);
            let chunk = &bytes[offset..end];
            state.output_bytes = state.output_bytes.saturating_add(chunk.len() as u64);
            let chunk = String::from_utf8_lossy(chunk).into_owned();
            state.push(if stderr {
                ExecutionEventKindV1::Stderr { chunk }
            } else {
                ExecutionEventKindV1::Stdout { chunk }
            })?;
            offset = end;
            if offset < bytes.len() && state.output_bytes >= budget {
                return Err(output_limit_error());
            }
        }
    }
    Ok(())
}
