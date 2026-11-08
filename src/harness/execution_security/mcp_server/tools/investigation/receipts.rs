use crate::core::investigation_broker::{
    InvestigationAuthorizationV1, InvestigationBroker, InvestigationEvidenceEnvelopeV1,
    InvestigationReceiptV1, InvestigationRequestV1,
};

pub(super) fn receipt_count(authorization: &InvestigationAuthorizationV1) -> Result<usize, String> {
    let entries = std::fs::read_dir(&authorization.receipt_root)
        .map_err(|_| "Manager receipt directory could not be scanned".to_owned())?;
    let mut count = 0;
    for entry in entries {
        let entry =
            entry.map_err(|_| "Manager receipt directory could not be scanned".to_owned())?;
        if entry.file_name().to_string_lossy().ends_with(".json") {
            count += 1;
        }
    }
    Ok(count)
}

pub(super) fn persist_receipt(
    authorization: &InvestigationAuthorizationV1,
    envelope: &InvestigationEvidenceEnvelopeV1,
) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(&authorization.receipt_root)
        .map_err(|_| "Manager receipt handoff is unavailable".to_owned())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("Manager receipt handoff is not a private directory".into());
    }
    let path = authorization
        .receipt_root
        .join(format!("{}.json", envelope.request.idempotency_key));
    let bytes = serde_json::to_vec(envelope)
        .map_err(|_| "investigation receipt could not be encoded".to_owned())?;
    if bytes.len() > 300_000 {
        return Err("investigation receipt exceeded the handoff size limit".into());
    }
    let temporary = authorization.receipt_root.join(format!(
        ".{}.{}.tmp",
        envelope.request.idempotency_key,
        uuid::Uuid::new_v4().hyphenated()
    ));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|_| "investigation receipt could not be handed back to Manager".to_owned())?;
    use std::io::Write;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| "investigation receipt could not be handed back to Manager".to_owned())?;
    std::fs::rename(&temporary, &path).map_err(|_| {
        "investigation receipt could not be committed to Manager handoff".to_owned()
    })?;
    std::fs::File::open(&authorization.receipt_root)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| "Manager receipt handoff could not be synchronized".to_owned())
}

pub(super) fn load_cached_receipt(
    authorization: &InvestigationAuthorizationV1,
    request: &InvestigationRequestV1,
) -> Result<Option<InvestigationReceiptV1>, String> {
    let path = authorization
        .receipt_root
        .join(format!("{}.json", request.idempotency_key));
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("cached Manager receipt metadata is unavailable".into()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > 300_000 {
        return Err("cached Manager receipt is not a bounded regular file".into());
    }
    let bytes =
        std::fs::read(&path).map_err(|_| "cached Manager receipt could not be read".to_owned())?;
    let envelope: InvestigationEvidenceEnvelopeV1 = serde_json::from_slice(&bytes)
        .map_err(|_| "cached Manager receipt is malformed".to_owned())?;
    if envelope.request != *request {
        return Err("idempotency key was replayed with a different request".into());
    }
    let context = authorization.context_for_request(request)?;
    InvestigationBroker::default().verify_receipt(&context, request, &envelope.receipt)?;
    Ok(Some(envelope.receipt))
}

pub(super) fn cleanup_temporary_receipts(
    authorization: &InvestigationAuthorizationV1,
) -> Result<(), String> {
    let entries = std::fs::read_dir(&authorization.receipt_root)
        .map_err(|_| "Manager receipt directory could not be scanned".to_owned())?;
    for entry in entries {
        let entry =
            entry.map_err(|_| "Manager receipt directory could not be scanned".to_owned())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with('.') || !name.ends_with(".tmp") {
            continue;
        }
        let parts = name
            .trim_start_matches('.')
            .strip_suffix(".tmp")
            .unwrap_or_default();
        let mut parts = parts.split('.');
        let request_id = parts.next().unwrap_or_default();
        let temp_id = parts.next().unwrap_or_default();
        if parts.next().is_some()
            || uuid::Uuid::parse_str(request_id).is_err()
            || uuid::Uuid::parse_str(temp_id).is_err()
        {
            continue;
        }
        let file_type = entry
            .file_type()
            .map_err(|_| "Manager receipt metadata is unavailable".to_owned())?;
        if file_type.is_file() && !file_type.is_symlink() {
            std::fs::remove_file(entry.path())
                .map_err(|_| "stale Manager receipt could not be removed".to_owned())?;
        }
    }
    Ok(())
}
