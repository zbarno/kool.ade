use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use super::record::{JournalEntry, RequestIndexEntry};

const MAX_JOURNAL_BYTES: u64 = 32 * 1024 * 1024;
const MAX_JOURNAL_ENTRIES: usize = 16_384;

pub(super) fn read_entries(log: &mut File) -> io::Result<Vec<JournalEntry>> {
    log.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    log.take(MAX_JOURNAL_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_JOURNAL_BYTES {
        return Err(invalid_data(
            "remote execution request journal exceeds its limit",
        ));
    }
    let mut entries = Vec::new();
    for line in bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        let entry: JournalEntry = serde_json::from_slice(line)
            .map_err(|_| invalid_data("remote execution request journal is corrupt"))?;
        if !is_canonical_uuid(&entry.request_id)
            || !is_sha256(&entry.request_sha256)
            || entry.created_unix_ms == 0
        {
            return Err(invalid_data(
                "remote execution request journal entry is invalid",
            ));
        }
        entries.push(entry);
    }
    if entries.len() > MAX_JOURNAL_ENTRIES * 2 {
        return Err(invalid_data(
            "remote execution request journal has too many entries",
        ));
    }
    Ok(entries)
}

pub(super) fn append_entry(log: &mut File, entry: JournalEntry) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(&entry)
        .map_err(|_| invalid_data("remote execution request journal entry could not be encoded"))?;
    bytes.push(b'\n');
    append_line(log, &bytes, "remote execution request journal")
}

pub(super) fn read_index(index: &mut File) -> io::Result<Vec<RequestIndexEntry>> {
    index.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    index.take(MAX_JOURNAL_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_JOURNAL_BYTES {
        return Err(invalid_data("remote request index exceeds its limit"));
    }
    let mut entries = Vec::new();
    for line in bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        let entry: RequestIndexEntry = serde_json::from_slice(line)
            .map_err(|_| invalid_data("remote request index is corrupt"))?;
        if !is_canonical_uuid(&entry.request_id)
            || !is_canonical_uuid(&entry.execution_id)
            || !is_sha256(&entry.request_sha256)
            || entry.created_unix_ms == 0
        {
            return Err(invalid_data("remote request index entry is invalid"));
        }
        entries.push(entry);
    }
    if entries.len() > MAX_JOURNAL_ENTRIES {
        return Err(invalid_data("remote request index has too many entries"));
    }
    Ok(entries)
}

pub(super) fn append_index(index: &mut File, entry: RequestIndexEntry) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(&entry)
        .map_err(|_| invalid_data("remote request index entry could not be encoded"))?;
    bytes.push(b'\n');
    append_line(index, &bytes, "remote request index")
}

fn append_line(file: &mut File, bytes: &[u8], label: &'static str) -> io::Result<()> {
    if file.metadata()?.len().saturating_add(bytes.len() as u64) > MAX_JOURNAL_BYTES {
        return Err(invalid_data(
            "remote execution request journal exceeds its limit",
        ));
    }
    file.seek(SeekFrom::End(0))?;
    file.write_all(bytes)?;
    file.sync_data()
        .map_err(|error| io::Error::new(error.kind(), format!("{label} could not be synchronized")))
}

pub(super) fn fingerprint(
    request: &super::super::super::protocol::EnvelopeV1,
) -> io::Result<String> {
    let bytes = serde_json::to_vec(request)
        .map_err(|_| invalid_data("remote request could not be fingerprinted"))?;
    use sha2::Digest as _;
    Ok(format!("{:x}", sha2::Sha256::digest(bytes)))
}

pub(super) fn open_private(path: &Path, create: bool) -> io::Result<File> {
    reject_symlink(path)?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(create);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    Ok(file)
}

pub(super) fn create_private_dir(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    reject_symlink(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn reject_symlink(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(invalid_data("remote execution journal path is a symlink"))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

pub(super) fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

pub(super) fn is_canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.hyphenated().to_string() == value)
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(super) fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
