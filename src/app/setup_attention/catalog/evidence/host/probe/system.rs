use std::{fs::File, io::Read, path::Path};

use super::{EntryKind, EntryMetadata, ProbeRead, ProbeSource};

const MAX_SYSTEM_FILE_BYTES: u64 = 256 * 1024;

pub(super) struct SystemProbeSource;

impl ProbeSource for SystemProbeSource {
    fn read_text(&self, path: &str) -> ProbeRead<String> {
        read_bounded_text(Path::new(path), MAX_SYSTEM_FILE_BYTES)
    }

    fn metadata(&self, path: &str) -> ProbeRead<EntryMetadata> {
        use std::os::unix::fs::{FileTypeExt, MetadataExt};

        let metadata = match std::fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) => return error_result(error.kind()),
        };
        let file_type = metadata.file_type();
        let kind = if file_type.is_dir() {
            EntryKind::Directory
        } else if file_type.is_socket() {
            EntryKind::Socket
        } else if file_type.is_file() {
            EntryKind::RegularFile
        } else {
            EntryKind::Other
        };
        ProbeRead::Found(EntryMetadata {
            kind,
            owner_uid: metadata.uid(),
            mode: metadata.mode(),
            symlink: file_type.is_symlink(),
        })
    }

    fn executable(&self, name: &str) -> ProbeRead<bool> {
        use std::os::unix::fs::PermissionsExt;

        let mut directories = vec![
            "/usr/bin".into(),
            "/usr/sbin".into(),
            "/bin".into(),
            "/sbin".into(),
            "/usr/local/bin".into(),
            "/usr/local/sbin".into(),
        ];
        if let Some(path) = std::env::var_os("PATH") {
            directories.extend(std::env::split_paths(&path).take(64));
        }
        directories.sort();
        directories.dedup();

        let mut inaccessible = false;
        let mut malformed = false;
        for directory in directories {
            let path = directory.join(name);
            match std::fs::metadata(path) {
                Ok(metadata) => {
                    let is_executable = metadata.file_type().is_file()
                        && metadata.permissions().mode() & 0o111 != 0;
                    if is_executable {
                        return ProbeRead::Found(true);
                    }
                    malformed = true;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                    inaccessible = true;
                }
                Err(_) => inaccessible = true,
            }
        }
        if inaccessible {
            ProbeRead::Inaccessible
        } else if malformed {
            ProbeRead::Malformed
        } else {
            ProbeRead::Missing
        }
    }
}

pub(super) fn read_bounded_text(path: &Path, limit: u64) -> ProbeRead<String> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) => return read_error(error.kind()),
    };
    let mut contents = String::new();
    let mut bounded = file.by_ref().take(limit + 1);
    match bounded.read_to_string(&mut contents) {
        Ok(_) if contents.len() as u64 <= limit => ProbeRead::Found(contents),
        Ok(_) => ProbeRead::Malformed,
        Err(error) if error.kind() == std::io::ErrorKind::InvalidData => ProbeRead::Malformed,
        Err(error) => read_error(error.kind()),
    }
}

fn read_error(kind: std::io::ErrorKind) -> ProbeRead<String> {
    error_result(kind)
}

fn error_result<T>(kind: std::io::ErrorKind) -> ProbeRead<T> {
    match kind {
        std::io::ErrorKind::NotFound => ProbeRead::Missing,
        std::io::ErrorKind::InvalidData => ProbeRead::Malformed,
        std::io::ErrorKind::PermissionDenied => ProbeRead::Inaccessible,
        _ => ProbeRead::Inaccessible,
    }
}
