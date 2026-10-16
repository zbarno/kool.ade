use super::*;
use crate::execution::{backend::BackendProbeV1, contract::ExecutionTarget};

pub(super) fn dispatch(
    backend: Arc<dyn ExecutionBackend>,
    runtime: &tokio::runtime::Runtime,
    capacity: Option<&crate::execution::remote::WorkerCapacityPool>,
    system_image_capability_key: &mut Option<[u8; 32]>,
    envelope: &EnvelopeV1,
) -> Result<(Kind, Value), ExecutionError> {
    let request: HealthRequest = decode(&envelope.payload)?;
    let control_id = "00000000-0000-4000-8000-000000000000";
    let session_id = crate::execution::remote::protocol::SYSTEM_IMAGE_SESSION_ID_V1;
    if envelope.execution_id != control_id
        && !(envelope.execution_id == session_id && request.system_image_key.is_some())
    {
        return Err(super::super::incompatible(
            "health request is outside the control session",
        ));
    }
    if request.wire_version != super::super::super::protocol::PROTOCOL_VERSION_V1 {
        return Err(super::super::incompatible(
            "remote worker protocol version is unsupported",
        ));
    }
    if let Some(encoded_key) = request.system_image_key {
        let key = decode_system_image_key(&encoded_key)
            .ok_or_else(|| super::super::incompatible("system image capability key is invalid"))?;
        if system_image_capability_key.is_some_and(|current| current != key) {
            return Err(super::super::incompatible(
                "system image capability key changed during the worker session",
            ));
        }
        *system_image_capability_key = Some(key);
    }
    let mut probe: BackendProbeV1 = runtime.block_on(backend.probe(&ExecutionTarget::local()))?;
    if let (Some(capabilities), Some(pool)) = (&mut probe.capabilities, capacity) {
        capabilities.maximum_execution_count = Some(pool.max_jobs());
    }
    Ok((
        Kind::Acknowledge,
        json!({"wire_version": request.wire_version, "probe": probe}),
    ))
}

fn decode_system_image_key(encoded: &str) -> Option<[u8; 32]> {
    if encoded.len() != 64 || !encoded.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let mut key = [0_u8; 32];
    for (index, pair) in encoded.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        key[index] = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(key)
}
