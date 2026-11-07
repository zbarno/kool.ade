use std::{
    io::{self, Read},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use crate::app::setup_attention::catalog::LinuxRootlessRecipeV1;

const APT_GET: &str = "/usr/bin/apt-get";
const MAX_PLAN_BYTES: usize = 256 * 1024;
const MAX_PLAN_PACKAGES: usize = 256;
const PLAN_TIMEOUT: Duration = Duration::from_secs(30);

pub(super) fn check_dependency_closure(recipe: LinuxRootlessRecipeV1) -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    {
        let deadline = Instant::now() + super::CLOSURE_TIMEOUT;
        let output = simulate_qemu_install()?;
        check_simulated_dependency_closure(&output, recipe, |package, version, recipe| {
            super::check_qemu_candidate_version_until(package, version, recipe, deadline)
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = recipe;
        anyhow::bail!("Linux QEMU dependency validation is Linux-only")
    }
}

fn check_simulated_dependency_closure(
    output: &[u8],
    recipe: LinuxRootlessRecipeV1,
    mut check_version: impl FnMut(&str, &str, LinuxRootlessRecipeV1) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    for (package, version) in parse_install_plan(output)? {
        check_version(&package, &version, recipe).map_err(|_| {
            anyhow::anyhow!(
                "QEMU dependency closure includes unapproved package {package} {version}"
            )
        })?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn simulate_qemu_install() -> anyhow::Result<Vec<u8>> {
    validate_apt_get_binary()?;
    use crate::app::bootstrap_remediation::TrustedArgument as A;
    let arguments = A::apt_source_options()
        .into_iter()
        .chain([
            A::AptSimulate,
            A::AptAssumeYes,
            A::AptNoUpgrade,
            A::AptNoRemove,
            A::AptNoInstallRecommends,
            A::AptInstall,
            A::AptQemuSystemX86,
            A::AptQemuUtils,
        ])
        .map(|argument| argument.as_os_string())
        .collect::<Vec<_>>();
    run_bounded(arguments)
}

#[cfg(target_os = "linux")]
fn validate_apt_get_binary() -> anyhow::Result<()> {
    use std::{os::unix::fs::MetadataExt, path::Path};

    let path = Path::new(APT_GET);
    anyhow::ensure!(
        path.canonicalize()? == path,
        "APT executable path is redirected"
    );
    let metadata = std::fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_file()
            && metadata.uid() == 0
            && metadata.mode() & 0o022 == 0
            && metadata.mode() & 0o111 != 0,
        "APT executable is not a trusted root-owned binary"
    );
    for directory in ["/", "/usr", "/usr/bin"] {
        let metadata = std::fs::symlink_metadata(directory)?;
        anyhow::ensure!(
            metadata.is_dir() && metadata.uid() == 0 && metadata.mode() & 0o022 == 0,
            "APT executable parent is not a trusted root-owned directory"
        );
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn run_bounded(arguments: Vec<std::ffi::OsString>) -> anyhow::Result<Vec<u8>> {
    use std::os::unix::process::CommandExt;

    let mut command = Command::new(APT_GET);
    command
        .args(arguments)
        .env_clear()
        .env("LANG", "C")
        .env("LC_ALL", "C")
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0);
    let mut child = command.spawn()?;
    let pid = child.id() as i32;
    let Some(stdout) = child.stdout.take() else {
        kill_group(pid);
        let _ = child.wait();
        anyhow::bail!("APT simulation output is unavailable")
    };
    let reader = match thread::Builder::new()
        .name("qemu-apt-plan".into())
        .spawn(move || retain_bounded(stdout))
    {
        Ok(reader) => reader,
        Err(error) => {
            kill_group(pid);
            let _ = child.wait();
            return Err(error.into());
        }
    };
    let deadline = Instant::now() + PLAN_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Err(error) => {
                kill_group(pid);
                let _ = child.wait();
                let _ = reader.join();
                return Err(error.into());
            }
            Ok(Some(status)) if reader.is_finished() => break status,
            Ok(Some(_)) if Instant::now() >= deadline => {
                kill_group(pid);
                let _ = child.wait();
                let _ = reader.join();
                anyhow::bail!("APT dependency simulation timed out")
            }
            Ok(Some(_)) => thread::sleep(Duration::from_millis(10)),
            Ok(None) if Instant::now() >= deadline => {
                kill_group(pid);
                let _ = child.wait();
                let _ = reader.join();
                anyhow::bail!("APT dependency simulation timed out")
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
        }
    };
    let (bytes, overflowed) = reader
        .join()
        .map_err(|_| anyhow::anyhow!("APT dependency output reader failed"))??;
    anyhow::ensure!(
        status.success() && !overflowed,
        "APT dependency simulation failed or exceeded its output bound"
    );
    Ok(bytes)
}

#[cfg(target_os = "linux")]
fn retain_bounded(mut reader: impl Read) -> io::Result<(Vec<u8>, bool)> {
    let mut retained = Vec::with_capacity(MAX_PLAN_BYTES);
    let mut overflowed = false;
    let mut buffer = [0u8; 4096];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let remaining = MAX_PLAN_BYTES.saturating_sub(retained.len());
        retained.extend_from_slice(&buffer[..read.min(remaining)]);
        overflowed |= read > remaining;
    }
    Ok((retained, overflowed))
}

#[cfg(target_os = "linux")]
fn kill_group(pid: i32) {
    // SAFETY: the APT simulator is launched in its own process group.
    unsafe {
        libc::kill(-pid, libc::SIGKILL);
    }
}

fn parse_install_plan(output: &[u8]) -> anyhow::Result<Vec<(String, String)>> {
    anyhow::ensure!(output.len() <= MAX_PLAN_BYTES, "APT plan is oversized");
    let text = std::str::from_utf8(output)?;
    let mut packages = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for line in text.lines().map(str::trim) {
        if line.starts_with("Remv ") {
            anyhow::bail!("APT plan attempts to remove a package");
        }
        if !line.starts_with("Inst ") {
            continue;
        }
        let fields = line.split_ascii_whitespace().collect::<Vec<_>>();
        let package = fields.get(1).copied().unwrap_or_default();
        anyhow::ensure!(
            valid_package_name(package),
            "APT plan has an invalid package name"
        );
        let mut version_index = 2;
        if fields
            .get(version_index)
            .is_some_and(|field| field.starts_with('['))
        {
            version_index += 1;
        }
        let version_field = fields
            .get(version_index)
            .ok_or_else(|| anyhow::anyhow!("APT plan has no selected package version"))?;
        anyhow::ensure!(
            *version_field == "(" || version_field.starts_with('('),
            "APT plan has malformed package version syntax"
        );
        let version = if *version_field == "(" {
            fields.get(version_index + 1).copied().unwrap_or_default()
        } else {
            version_field.strip_prefix('(').unwrap_or(version_field)
        };
        anyhow::ensure!(
            valid_version(version),
            "APT plan has an invalid package version"
        );
        anyhow::ensure!(seen.insert(package), "APT plan repeats a package");
        packages.push((package.to_owned(), version.to_owned()));
        anyhow::ensure!(
            packages.len() <= MAX_PLAN_PACKAGES,
            "APT plan has too many packages"
        );
    }
    Ok(packages)
}

fn valid_package_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"+.-:".contains(&byte)
        })
}

fn valid_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 160
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"+~:.-".contains(&byte))
}

#[cfg(test)]
#[path = "qemu_closure_tests.rs"]
mod tests;
