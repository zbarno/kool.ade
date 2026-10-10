use super::*;

pub(crate) fn recover_store(store: &PlanningStore) -> Result<bool, StoreError> {
    let transaction_root = transaction_root(store)?;
    let root = store_identity(store);
    let journal_path = journal_path(&transaction_root, &root);
    let _lock = acquire_lock(&transaction_root, &root)?;
    match std::fs::symlink_metadata(&journal_path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(StoreError::InvalidPath(journal_path.display().to_string()));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(source) => {
            return Err(StoreError::Io {
                path: journal_path,
                source,
            });
        }
    }
    let bytes = match std::fs::read(&journal_path) {
        Ok(bytes) => bytes,
        Err(source) => {
            return Err(StoreError::Io {
                path: journal_path,
                source,
            });
        }
    };
    let journal: Journal = serde_json::from_slice(&bytes)
        .map_err(|error| StoreError::MalformedState(error.to_string()))?;
    if journal.root != root {
        return Err(StoreError::MalformedState(
            "transaction journal belongs to a different planning store".into(),
        ));
    }
    restore(store, &journal.entries)?;
    remove_journal(&journal_path)?;
    Ok(true)
}
