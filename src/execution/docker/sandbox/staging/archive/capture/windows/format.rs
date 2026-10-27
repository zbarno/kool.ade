use std::path::Path;

use anyhow::Context;
use tar::{Builder, EntryType, Header};

pub(super) fn path_key(path: &str) -> String {
    path.replace('\\', "/")
        .chars()
        .flat_map(char::to_uppercase)
        .collect()
}

pub(super) fn mode_for(directory: bool, executable: bool, read_only: bool) -> u32 {
    let mode = if directory || executable {
        0o755
    } else {
        0o644
    };
    if read_only { mode & !0o222 } else { mode }
}

pub(super) fn append_header(
    builder: &mut Builder<Vec<u8>>,
    path: &Path,
    kind: EntryType,
    size: u64,
    mode: u32,
    bytes: &[u8],
) -> anyhow::Result<()> {
    let mut header = Header::new_gnu();
    header.set_entry_type(kind);
    header.set_size(size);
    header.set_mode(mode);
    header.set_uid(if mode & 0o200 != 0 { 10001 } else { 0 });
    header.set_gid(if mode & 0o200 != 0 { 10001 } else { 0 });
    header.set_mtime(0);
    header.set_cksum();
    builder
        .append_data(&mut header, path, bytes)
        .context("workspace archive entry could not be encoded")?;
    Ok(())
}
