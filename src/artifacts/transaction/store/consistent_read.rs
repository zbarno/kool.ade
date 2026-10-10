use super::{acquire_lock, journal_path, store_identity, transaction_root};
use crate::artifacts::planning_store::{PlanningStore, StoreError};

/// Run a collection read while holding the same per-store lock as writers.
/// This prevents loaders from observing a prefix of a journaled multi-file
/// transaction, including normalized-record migrations.
pub(crate) fn with_consistent_read<T, E>(
    store: &PlanningStore,
    read: impl FnOnce() -> Result<T, E>,
) -> Result<T, E>
where
    E: From<StoreError>,
{
    let transaction_root = transaction_root(store).map_err(E::from)?;
    let root = store_identity(store);
    let journal_path = journal_path(&transaction_root, &root);
    let _lock = acquire_lock(&transaction_root, &root).map_err(E::from)?;
    match std::fs::symlink_metadata(&journal_path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(StoreError::InvalidPath(journal_path.display().to_string()).into());
        }
        Ok(_) => {
            return Err(StoreError::UnavailableStore(
                "an unrecovered planning store transaction exists".into(),
            )
            .into());
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(StoreError::Io {
                path: journal_path,
                source,
            }
            .into());
        }
    }
    read()
}
