use std::path::Path;

use crate::artifacts::planning_store::StoreError;
use crate::project_runtime::RuntimeReceiptV1;

use super::{MAX_RECEIPT_BYTES, ProjectRuntimeStore, RECEIPTS_ROOT};

impl ProjectRuntimeStore<'_> {
    /// Appends immutable task evidence; an existing receipt is never replaced.
    pub fn append_receipt(&self, receipt: &RuntimeReceiptV1) -> Result<(), StoreError> {
        receipt.validate().map_err(StoreError::MalformedState)?;
        let receipt_id = uuid::Uuid::parse_str(&receipt.receipt_id)
            .map_err(|_| StoreError::InvalidPath(receipt.receipt_id.clone()))?;
        if receipt_id.hyphenated().to_string() != receipt.receipt_id {
            return Err(StoreError::InvalidPath(receipt.receipt_id.clone()));
        }
        let path = format!("{RECEIPTS_ROOT}/{receipt_id}.json");
        let encoded = serde_json::to_vec_pretty(receipt).map_err(|error| {
            StoreError::MalformedState(format!("runtime receipt cannot be encoded: {error}"))
        })?;
        if encoded.len() > MAX_RECEIPT_BYTES {
            return Err(StoreError::MalformedState(
                "runtime receipt exceeds the write limit".into(),
            ));
        }
        self.store.atomic_create(path, &encoded)
    }

    pub fn load_receipt(&self, receipt_id: &str) -> Result<Option<RuntimeReceiptV1>, StoreError> {
        let id = uuid::Uuid::parse_str(receipt_id)
            .map_err(|_| StoreError::InvalidPath(receipt_id.to_owned()))?;
        if id.hyphenated().to_string() != receipt_id {
            return Err(StoreError::InvalidPath(receipt_id.to_owned()));
        }
        let path = format!("{RECEIPTS_ROOT}/{id}.json");
        let bytes = match self.store.read(Path::new(&path)) {
            Ok(bytes) => bytes,
            Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        if bytes.len() > MAX_RECEIPT_BYTES {
            return Err(StoreError::MalformedState(
                "runtime receipt exceeds the read limit".into(),
            ));
        }
        let receipt: RuntimeReceiptV1 = serde_json::from_slice(&bytes).map_err(|error| {
            StoreError::MalformedState(format!("invalid runtime receipt: {error}"))
        })?;
        receipt.validate().map_err(StoreError::MalformedState)?;
        if receipt.receipt_id != id.hyphenated().to_string() {
            return Err(StoreError::MalformedState(
                "runtime receipt identity does not match its file name".into(),
            ));
        }
        Ok(Some(receipt))
    }

    /// Load validated immutable receipts for one exact task identity.
    pub fn receipts_for_task(&self, task_id: &str) -> Result<Vec<RuntimeReceiptV1>, StoreError> {
        if !super::super::recipe::valid_id(task_id) {
            return Err(StoreError::InvalidPath(task_id.to_owned()));
        }
        let paths = self.store.list_record_paths(RECEIPTS_ROOT)?;
        let mut receipts = Vec::new();
        for path in paths {
            let receipt_id = Path::new(&path)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .ok_or_else(|| StoreError::InvalidPath(path.clone()))?;
            let receipt = self
                .load_receipt(receipt_id)?
                .ok_or_else(|| StoreError::MalformedState(format!("receipt {path} disappeared")))?;
            if receipt.task_id == task_id {
                receipts.push(receipt);
            }
        }
        receipts.sort_by(|left, right| {
            left.observed_at_unix_ms
                .cmp(&right.observed_at_unix_ms)
                .then_with(|| left.receipt_id.cmp(&right.receipt_id))
        });
        Ok(receipts)
    }
}
