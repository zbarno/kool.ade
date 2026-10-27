use std::{
    fs::OpenOptions,
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
};

pub(super) const APP_UNIT_NAME: &str = "koolade-rootless-docker.service";
const APP_UNIT_TEMPLATE: &[u8] = b"[Unit]\nDescription=Kool.ad/e rootless Docker service\n\n[Service]\nEnvironment=PATH=/usr/bin:/sbin:/usr/sbin:/bin\nExecStart=/usr/bin/dockerd-rootless.sh\nType=notify\nDelegate=yes\nRestart=always\n\n[Install]\nWantedBy=default.target\n";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum UnitFileState {
    Missing,
    Exact,
    Conflict,
    Unknown,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct UnitProperties {
    pub(super) load_state: String,
    pub(super) active_state: String,
    pub(super) fragment_path: String,
}

pub(super) fn inspect_app_unit(path: &std::path::Path, uid: u32) -> UnitFileState {
    let path_metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return UnitFileState::Missing;
        }
        Err(_) => return UnitFileState::Unknown,
    };
    if path_metadata.file_type().is_symlink() || !safe_file(&path_metadata, uid) {
        return UnitFileState::Conflict;
    }
    let Ok(mut file) = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)
    else {
        return UnitFileState::Unknown;
    };
    let Ok(open_metadata) = file.metadata() else {
        return UnitFileState::Unknown;
    };
    if !same_file(&path_metadata, &open_metadata) || !safe_file(&open_metadata, uid) {
        return UnitFileState::Unknown;
    }
    let mut contents = Vec::new();
    if file
        .by_ref()
        .take((APP_UNIT_TEMPLATE.len() + 1) as u64)
        .read_to_end(&mut contents)
        .is_err()
    {
        return UnitFileState::Unknown;
    }
    let final_metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => return UnitFileState::Unknown,
    };
    if !same_file(&open_metadata, &final_metadata) {
        return UnitFileState::Unknown;
    }
    if contents == APP_UNIT_TEMPLATE {
        UnitFileState::Exact
    } else {
        UnitFileState::Conflict
    }
}

fn safe_file(metadata: &std::fs::Metadata, uid: u32) -> bool {
    metadata.is_file()
        && metadata.uid() == uid
        && metadata.nlink() == 1
        && metadata.mode() & 0o022 == 0
        && metadata.len() <= APP_UNIT_TEMPLATE.len() as u64
}

fn same_file(left: &std::fs::Metadata, right: &std::fs::Metadata) -> bool {
    left.dev() == right.dev() && left.ino() == right.ino()
}

pub(super) fn parse_properties(output: &str) -> Option<UnitProperties> {
    let mut load_state = None;
    let mut active_state = None;
    let mut fragment_path = None;
    for line in output.lines() {
        if let Some(value) = line.strip_prefix("LoadState=") {
            if load_state.replace(value.to_owned()).is_some() {
                return None;
            }
        } else if let Some(value) = line.strip_prefix("ActiveState=") {
            if active_state.replace(value.to_owned()).is_some() {
                return None;
            }
        } else if let Some(value) = line.strip_prefix("FragmentPath=")
            && fragment_path.replace(value.to_owned()).is_some()
        {
            return None;
        }
    }
    Some(UnitProperties {
        load_state: load_state?,
        active_state: active_state?,
        fragment_path: fragment_path?,
    })
}

pub(super) fn retryable(
    properties: &UnitProperties,
    exact_file: bool,
    expected_path: &std::path::Path,
) -> bool {
    if !exact_file {
        return properties.load_state == "not-found" && properties.active_state == "inactive";
    }
    (properties.load_state == "not-found" && properties.active_state == "inactive")
        || (properties.load_state == "loaded"
            && matches!(properties.active_state.as_str(), "inactive" | "failed")
            && properties.fragment_path == expected_path.to_string_lossy().as_ref())
}

pub(super) fn resumable_app_unit(
    file_state: UnitFileState,
    properties: &UnitProperties,
    expected_path: &std::path::Path,
) -> bool {
    match file_state {
        UnitFileState::Missing => retryable(properties, false, expected_path),
        UnitFileState::Exact => retryable(properties, true, expected_path),
        UnitFileState::Conflict | UnitFileState::Unknown => false,
    }
}
