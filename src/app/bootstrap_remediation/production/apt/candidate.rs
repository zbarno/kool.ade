use super::source::{read_bounded_file, verify_root_file_metadata};
use crate::app::bootstrap_remediation::TrustedArgument;
use crate::app::setup_attention::catalog::LinuxRootlessRecipeV1;

#[cfg(target_os = "linux")]
const APT_CACHE: &str = "/usr/bin/apt-cache";
#[cfg(target_os = "linux")]
const MAX_POLICY_BYTES: usize = 256 * 1024;

pub(super) fn check_docker_candidates(recipe: LinuxRootlessRecipeV1) -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    {
        let apt_cache = std::path::Path::new(APT_CACHE);
        let (_binary, metadata) = read_bounded_file(apt_cache)?;
        verify_root_file_metadata(&metadata)?;
        for package in [
            "docker-ce",
            "docker-ce-cli",
            "containerd.io",
            "docker-ce-rootless-extras",
        ] {
            let bytes = run_apt_cache_policy(apt_cache, package)?;
            let policy = std::str::from_utf8(&bytes)?;
            anyhow::ensure!(
                candidate_is_from_docker_recipe(policy, recipe),
                "apt-cache candidate for {package} is not from the exact signed Docker stable recipe"
            );
        }
        for package in ["uidmap", "dbus-user-session"] {
            let bytes = run_apt_cache_policy(apt_cache, package)?;
            let policy = std::str::from_utf8(&bytes)?;
            anyhow::ensure!(
                candidate_is_from_base_distribution(policy, recipe),
                "apt-cache candidate for {package} is not from an official signed distro archive"
            );
        }
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = recipe;
        anyhow::bail!("Linux rootless Docker package candidates are Linux-only")
    }
}

#[cfg(target_os = "linux")]
fn run_apt_cache_policy(path: &std::path::Path, package: &str) -> anyhow::Result<Vec<u8>> {
    use std::{
        io::Read,
        process::{Command, Stdio},
        thread,
        time::{Duration, Instant},
    };

    let mut child = Command::new(path)
        .args(apt_cache_arguments(package))
        .env_clear()
        .env("LANG", "C")
        .env("LC_ALL", "C")
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("apt-cache output pipe is unavailable"))?;
    let reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .by_ref()
            .take((MAX_POLICY_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        match child.try_wait()? {
            Some(status) => break status,
            None if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            None => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                anyhow::bail!("apt-cache package candidate inspection timed out")
            }
        }
    };
    let bytes = reader
        .join()
        .map_err(|_| anyhow::anyhow!("apt-cache output reader failed"))??;
    anyhow::ensure!(
        status.success() && bytes.len() <= MAX_POLICY_BYTES,
        "apt-cache could not report a bounded package candidate"
    );
    Ok(bytes)
}

#[cfg(target_os = "linux")]
fn apt_cache_arguments(package: &str) -> Vec<std::ffi::OsString> {
    let mut arguments = TrustedArgument::apt_source_options()
        .iter()
        .map(TrustedArgument::as_os_string)
        .collect::<Vec<_>>();
    arguments.push("policy".into());
    arguments.push(package.into());
    arguments
}

#[cfg(test)]
pub(super) fn apt_cache_arguments_for_test(package: &str) -> Vec<String> {
    #[cfg(target_os = "linux")]
    let arguments = apt_cache_arguments(package);
    #[cfg(not(target_os = "linux"))]
    let arguments = {
        let mut arguments = TrustedArgument::apt_source_options()
            .iter()
            .map(TrustedArgument::as_os_string)
            .collect::<Vec<_>>();
        arguments.extend(["policy".into(), package.into()]);
        arguments
    };
    arguments
        .iter()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect()
}

pub(super) fn candidate_is_from_docker_noble(policy: &str) -> bool {
    candidate_is_from_docker_recipe(policy, LinuxRootlessRecipeV1::Ubuntu2404Amd64)
}

pub(super) fn candidate_is_from_ubuntu_noble(policy: &str) -> bool {
    candidate_is_from_base_distribution(policy, LinuxRootlessRecipeV1::Ubuntu2404Amd64)
}

pub(super) fn candidate_is_from_docker_recipe(policy: &str, recipe: LinuxRootlessRecipeV1) -> bool {
    candidate_is_from_policy(policy, |fields| is_docker_origin(fields, recipe))
}

pub(super) fn candidate_is_from_base_distribution(
    policy: &str,
    recipe: LinuxRootlessRecipeV1,
) -> bool {
    candidate_is_from_policy(policy, |fields| is_base_origin(fields, recipe))
}

fn candidate_is_from_policy(policy: &str, approved_origin: impl Fn(&[&str]) -> bool) -> bool {
    let mut candidate = None::<&str>;
    let mut in_version_table = false;
    let mut candidate_version = false;
    let mut candidate_has_approved_origin = false;
    let mut candidate_has_foreign_origin = false;
    for line in policy.lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("Candidate:") {
            if candidate.is_some() {
                return false;
            }
            let value = value.trim();
            if value.is_empty() || value == "(none)" {
                return false;
            }
            candidate = Some(value);
            continue;
        }
        if trimmed == "Version table:" {
            if in_version_table {
                return false;
            }
            in_version_table = true;
            continue;
        }
        if !in_version_table || trimmed.is_empty() {
            continue;
        }
        let fields = trimmed.split_whitespace().collect::<Vec<_>>();
        let version_index = if fields.first() == Some(&"***") { 1 } else { 0 };
        if let (Some(version), Some(priority)) =
            (fields.get(version_index), fields.get(version_index + 1))
            && (*priority == "-" || priority.parse::<i32>().is_ok())
        {
            candidate_version = candidate == Some(*version);
            continue;
        }
        if candidate_version && fields.len() >= 2 && fields[0].parse::<i32>().is_ok() {
            let installed_status = fields[1] == "/var/lib/dpkg/status";
            let approved = approved_origin(&fields);
            candidate_has_approved_origin |= approved;
            candidate_has_foreign_origin |= !approved && !installed_status;
        }
    }
    candidate.is_some()
        && in_version_table
        && candidate_has_approved_origin
        && !candidate_has_foreign_origin
}

fn is_docker_origin(fields: &[&str], recipe: LinuxRootlessRecipeV1) -> bool {
    let (uri, suite) = match recipe {
        LinuxRootlessRecipeV1::Debian13Amd64 => {
            ("https://download.docker.com/linux/debian", "trixie/stable")
        }
        LinuxRootlessRecipeV1::Ubuntu2404Amd64 | LinuxRootlessRecipeV1::PopOs2404Amd64 => {
            ("https://download.docker.com/linux/ubuntu", "noble/stable")
        }
        LinuxRootlessRecipeV1::Fedora45Amd64 => return false,
    };
    fields.len() >= 5
        && fields[1] == uri
        && fields[2] == suite
        && fields[3] == "amd64"
        && fields[4] == "Packages"
}

fn is_base_origin(fields: &[&str], recipe: LinuxRootlessRecipeV1) -> bool {
    if fields.len() < 5 || fields[3] != "amd64" || fields[4] != "Packages" {
        return false;
    }
    match recipe {
        LinuxRootlessRecipeV1::Debian13Amd64 => match fields[1] {
            "http://deb.debian.org/debian" | "https://deb.debian.org/debian" => {
                matches!(fields[2], "trixie/main" | "trixie-updates/main")
            }
            "http://security.debian.org/debian-security"
            | "https://security.debian.org/debian-security" => fields[2] == "trixie-security/main",
            _ => false,
        },
        LinuxRootlessRecipeV1::Ubuntu2404Amd64 => match fields[1] {
            "http://archive.ubuntu.com/ubuntu" | "https://archive.ubuntu.com/ubuntu" => {
                matches!(
                    fields[2],
                    "noble/main"
                        | "noble/universe"
                        | "noble-updates/main"
                        | "noble-updates/universe"
                        | "noble-backports/main"
                        | "noble-backports/universe"
                )
            }
            "http://security.ubuntu.com/ubuntu" | "https://security.ubuntu.com/ubuntu" => {
                matches!(fields[2], "noble-security/main" | "noble-security/universe")
            }
            _ => false,
        },
        LinuxRootlessRecipeV1::PopOs2404Amd64 => {
            let pop_ubuntu = fields[1] == "http://apt.pop-os.org/ubuntu"
                && matches!(
                    fields[2],
                    "noble/main"
                        | "noble/universe"
                        | "noble-updates/main"
                        | "noble-updates/universe"
                        | "noble-security/main"
                        | "noble-security/universe"
                        | "noble-backports/main"
                        | "noble-backports/universe"
                );
            let pop_release = matches!(
                fields[1],
                "http://apt.pop-os.org/release" | "http://apt.pop-os.org/proprietary"
            ) && fields[2] == "noble/main";
            pop_ubuntu || pop_release
        }
        LinuxRootlessRecipeV1::Fedora45Amd64 => false,
    }
}
