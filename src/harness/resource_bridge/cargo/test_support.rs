use super::{SharedPreparationOperations, prepare};
use std::{path::Path, sync::Arc};

pub(in crate::harness::resource_bridge) fn test_preparation_operations<F>(
    fetch: F,
) -> Arc<SharedPreparationOperations>
where
    F: Fn(&Path, &Path, &Path, usize) -> anyhow::Result<usize> + Send + Sync + 'static,
{
    let fetch = Arc::new(move |worktree: &Path, cargo: &Path, cache: &Path, limit| {
        match fetch(worktree, cargo, cache, limit) {
            Ok(bytes_received) => prepare::FetchOutcome {
                bytes_received,
                result: Ok(()),
            },
            Err(error) => prepare::FetchOutcome {
                bytes_received: 0,
                result: Err(error),
            },
        }
    });
    Arc::new(SharedPreparationOperations { fetch })
}
