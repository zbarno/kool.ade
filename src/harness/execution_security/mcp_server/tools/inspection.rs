use serde_json::{Value, json};
use sha2::Digest;
use std::{
    fs::{self, File, OpenOptions},
    io,
    path::Path,
};

use super::{CallResponse, InspectionAudit, ServerConfig};

const MAX_REVIEW_SCREENSHOT_BYTES: u64 = 16 * 1024 * 1024;

pub(super) fn screenshot(config: &ServerConfig, arguments: &Value) -> CallResponse {
    let response = (|| -> Result<(Value, String), String> {
        let access = config
            .screenshot_access
            .as_ref()
            .ok_or_else(|| "runtime QA screenshot access is unavailable".to_owned())?;
        let id = arguments["screenshot_id"]
            .as_str()
            .ok_or_else(|| "a screenshot ID is required".to_owned())?;
        let expected_hash = access
            .sha256_by_id
            .get(id)
            .ok_or_else(|| "screenshot is not part of this runtime QA review".to_owned())?;
        let path = access.root.join(format!("{id}.png"));
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|_| "runtime QA screenshot image is missing".to_owned())?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err("runtime QA screenshot image is not a regular file".into());
        }
        if metadata.len() > MAX_REVIEW_SCREENSHOT_BYTES {
            return Err("runtime QA screenshot exceeds the visual review size limit".into());
        }
        let bytes = std::fs::read(path)
            .map_err(|_| "runtime QA screenshot image cannot be read".to_owned())?;
        if bytes.len() as u64 > MAX_REVIEW_SCREENSHOT_BYTES
            || !bytes.starts_with(b"\x89PNG\r\n\x1a\n")
            || format!("{:x}", sha2::Sha256::digest(&bytes)) != *expected_hash
            || image::load_from_memory(&bytes).is_err()
        {
            return Err("runtime QA screenshot failed hash or PNG validation".into());
        }
        let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes);
        Ok((
            json!({
                "content": [
                    { "type": "text", "text": format!("Visual evidence screenshot {id} from the validated current runtime QA run.") },
                    { "type": "image", "data": encoded, "mimeType": "image/png" }
                ],
                "isError": false
            }),
            id.to_owned(),
        ))
    })();
    match response {
        Ok((response, screenshot_id)) => CallResponse {
            response,
            after_delivery: config
                .screenshot_access
                .as_ref()
                .map(|access| InspectionAudit {
                    audit_root: access.audit_root.clone(),
                    screenshot_id,
                }),
        },
        Err(error) => CallResponse {
            response: json!({ "content": [{ "type": "text", "text": error }], "isError": true }),
            after_delivery: None,
        },
    }
}

pub(super) fn record_inspection(audit_root: &Path, id: &str) -> Result<(), String> {
    record_inspection_with(audit_root, id, |file| file.sync_all())
}

fn record_inspection_with(
    audit_root: &Path,
    id: &str,
    sync: impl FnOnce(&File) -> io::Result<()>,
) -> Result<(), String> {
    let marker = audit_root.join(format!("{id}.viewed"));
    let pending = audit_root.join(format!("{id}.{}.pending", uuid::Uuid::new_v4()));
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&pending)
        .map_err(|_| "runtime QA screenshot inspection could not be recorded".to_owned())?;
    if sync(&file).is_err() {
        drop(file);
        let _ = fs::remove_file(&pending);
        return Err("runtime QA screenshot inspection could not be recorded".into());
    }
    drop(file);
    if fs::rename(&pending, &marker).is_err() {
        let _ = fs::remove_file(&pending);
        let existing_marker_is_valid = fs::symlink_metadata(&marker).is_ok_and(|metadata| {
            metadata.is_file() && !metadata.file_type().is_symlink() && metadata.len() == 0
        });
        return if existing_marker_is_valid {
            Ok(())
        } else {
            Err("runtime QA screenshot inspection could not be recorded".into())
        };
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_failure_removes_staged_inspection_marker() {
        let directory = tempfile::tempdir().unwrap();
        let error = record_inspection_with(directory.path(), "screenshot-id", |_| {
            Err(io::Error::other("synthetic sync failure"))
        });

        assert!(error.is_err());
        assert!(!directory.path().join("screenshot-id.viewed").exists());
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
    }
}
