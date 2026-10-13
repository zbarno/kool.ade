use std::fs::Metadata;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct WorkspaceObjectId {
    volume: u64,
    index: u64,
}

#[cfg(unix)]
pub(super) fn from_metadata(metadata: &Metadata) -> Option<WorkspaceObjectId> {
    use std::os::unix::fs::MetadataExt;

    Some(WorkspaceObjectId {
        volume: metadata.dev(),
        index: metadata.ino(),
    })
}

#[cfg(not(unix))]
pub(super) fn from_metadata(_metadata: &Metadata) -> Option<WorkspaceObjectId> {
    None
}
