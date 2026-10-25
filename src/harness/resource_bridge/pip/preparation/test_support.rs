use super::{DownloadOutcome, SharedPreparationOperations};
use std::{path::Path, sync::Arc};

pub(in crate::harness::resource_bridge) fn test_preparation_operations<F>(
    download: F,
) -> Arc<SharedPreparationOperations>
where
    F: Fn(&Path, &Path, usize) -> anyhow::Result<usize> + Send + Sync + 'static,
{
    let download = Arc::new(move |requirements: &Path, wheelhouse: &Path, limit| {
        match download(requirements, wheelhouse, limit) {
            Ok(bytes_received) => DownloadOutcome {
                bytes_received,
                result: Ok(()),
            },
            Err(error) => DownloadOutcome {
                bytes_received: 0,
                result: Err(error),
            },
        }
    });
    Arc::new(SharedPreparationOperations { download })
}
