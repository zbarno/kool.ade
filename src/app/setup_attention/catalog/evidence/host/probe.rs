use std::{fs::File, io::Read, path::Path};

use crate::app::setup_attention::catalog::EvidenceState as State;
use crate::app::setup_attention::catalog::{
    LinuxDistributionV1 as Distribution, LinuxDistributionVersionV1 as Version,
};

#[cfg(target_os = "linux")]
use std::os::unix::fs::FileTypeExt;

const MAX_SYSTEM_FILE_BYTES: u64 = 256 * 1024;
const MIN_SUBID_RANGE: u64 = 65_536;

pub(super) fn linux_distribution() -> Option<(Distribution, Option<Version>)> {
    bounded_text(Path::new("/etc/os-release"))
        .and_then(|contents| parse_os_release_distribution(&contents))
}

pub(super) fn linux_uid_mappings() -> State {
    #[cfg(target_os = "linux")]
    {
        let uid = unsafe { libc::geteuid() };
        let user = current_username(uid);
        let Some(subuid) = bounded_text(Path::new("/etc/subuid")) else {
            return State::Unknown;
        };
        let Some(subgid) = bounded_text(Path::new("/etc/subgid")) else {
            return State::Unknown;
        };
        if has_subid_range(&subuid, uid, user.as_deref())
            && has_subid_range(&subgid, uid, user.as_deref())
        {
            State::Ready
        } else {
            State::Missing
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        State::Unknown
    }
}

pub(super) fn linux_service_manager() -> State {
    #[cfg(target_os = "linux")]
    {
        let uid = unsafe { libc::geteuid() };
        let path = format!("/run/user/{uid}/systemd/private");
        if std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_socket()) {
            State::Ready
        } else {
            State::Unknown
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        State::Unknown
    }
}

pub(super) fn linux_package_manager() -> State {
    if has_package_manager() {
        State::Ready
    } else {
        State::Unknown
    }
}

fn bounded_text(path: &Path) -> Option<String> {
    let file = File::open(path).ok()?;
    let mut contents = String::new();
    file.take(MAX_SYSTEM_FILE_BYTES + 1)
        .read_to_string(&mut contents)
        .ok()?;
    (contents.len() as u64 <= MAX_SYSTEM_FILE_BYTES).then_some(contents)
}

fn parse_os_release_distribution(contents: &str) -> Option<(Distribution, Option<Version>)> {
    let id = os_release_value(contents, "ID")?;
    if id.is_empty()
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    {
        return None;
    }
    let distribution = match id.to_ascii_lowercase().as_str() {
        "ubuntu" => Distribution::Ubuntu,
        "debian" => Distribution::Debian,
        "fedora" => Distribution::Fedora,
        "arch" => Distribution::Arch,
        "opensuse" | "opensuse-leap" | "opensuse-tumbleweed" | "sles" => Distribution::Suse,
        "alpine" => Distribution::Alpine,
        "pop" => Distribution::PopOs,
        _ => Distribution::Other,
    };
    let version = os_release_value(contents, "VERSION_ID").and_then(parse_distribution_version);
    Some((distribution, version))
}

fn os_release_value<'a>(contents: &'a str, key: &str) -> Option<&'a str> {
    contents
        .lines()
        .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
        .map(str::trim)
        .map(|value| value.trim_matches(['"', '\'']))
}

fn parse_distribution_version(value: &str) -> Option<Version> {
    let (major, minor) = value.split_once('.').unwrap_or((value, "0"));
    if !major.bytes().all(|byte| byte.is_ascii_digit())
        || !minor.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    Some(Version {
        major: major.parse().ok()?,
        minor: minor.parse().ok()?,
    })
}

fn has_subid_range(contents: &str, uid: u32, user: Option<&str>) -> bool {
    let numeric_uid = uid.to_string();
    contents.lines().any(|line| {
        let mut fields = line.split(':');
        let Some(owner) = fields.next() else {
            return false;
        };
        let Some(start) = fields.next().and_then(|field| field.parse::<u64>().ok()) else {
            return false;
        };
        let Some(count) = fields.next().and_then(|field| field.parse::<u64>().ok()) else {
            return false;
        };
        (owner == numeric_uid || user == Some(owner))
            && count >= MIN_SUBID_RANGE
            && start.checked_add(count).is_some()
    })
}

#[cfg(target_os = "linux")]
fn current_username(uid: u32) -> Option<String> {
    let contents = bounded_text(Path::new("/etc/passwd"))?;
    contents.lines().find_map(|line| {
        let mut fields = line.split(':');
        let name = fields.next()?;
        let _password = fields.next()?;
        let entry_uid = fields.next()?.parse::<u32>().ok()?;
        (entry_uid == uid).then(|| name.to_owned())
    })
}

#[cfg(not(target_os = "linux"))]
fn current_username(_uid: u32) -> Option<String> {
    None
}

fn has_package_manager() -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).take(64).any(|directory| {
        ["apt-get", "dnf", "yum", "pacman", "zypper", "apk"]
            .into_iter()
            .any(|name| is_executable(&directory.join(name)))
    })
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
#[path = "probe_tests.rs"]
mod tests;
