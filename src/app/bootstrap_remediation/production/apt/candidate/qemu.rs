use std::time::{Duration, Instant};

use crate::app::setup_attention::catalog::LinuxRootlessRecipeV1;

use super::{candidate_is_from_base_distribution, run_apt_cache_policy_with_timeout};

#[path = "qemu_closure.rs"]
mod qemu_closure;

const CLOSURE_TIMEOUT: Duration = Duration::from_secs(60);
const POLICY_TIMEOUT: Duration = Duration::from_secs(10);

pub(in crate::app::bootstrap_remediation::production::apt) fn check_qemu_candidates(
    recipe: LinuxRootlessRecipeV1,
) -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    {
        super::validate_apt_cache_binary()?;
        let apt_cache = std::path::Path::new(super::APT_CACHE);
        for package in ["qemu-system-x86", "qemu-utils"] {
            let bytes = super::run_apt_cache_policy(apt_cache, package)?;
            let policy = std::str::from_utf8(&bytes)?;
            anyhow::ensure!(
                candidate_is_from_base_distribution(policy, recipe),
                "apt-cache candidate for {package} is not from the exact signed distro archive"
            );
        }
        qemu_closure::check_dependency_closure(recipe)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = recipe;
        anyhow::bail!("Linux QEMU package candidates are Linux-only")
    }
}

fn check_qemu_candidate_version_until(
    package: &str,
    version: &str,
    recipe: LinuxRootlessRecipeV1,
    deadline: Instant,
) -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    {
        super::validate_apt_cache_binary()?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        anyhow::ensure!(!remaining.is_zero(), "QEMU dependency validation timed out");
        let apt_cache = std::path::Path::new(super::APT_CACHE);
        let bytes =
            run_apt_cache_policy_with_timeout(apt_cache, package, remaining.min(POLICY_TIMEOUT))?;
        let policy = std::str::from_utf8(&bytes)?;
        anyhow::ensure!(
            candidate_version_is_from_base_distribution(policy, version, recipe),
            "APT selected {package} {version} outside the exact signed distro archives"
        );
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (package, version, recipe, deadline);
        anyhow::bail!("Linux QEMU package validation is Linux-only")
    }
}

pub(in crate::app::bootstrap_remediation::production::apt) fn candidate_version_is_from_base_distribution(
    policy: &str,
    expected_version: &str,
    recipe: LinuxRootlessRecipeV1,
) -> bool {
    let mut candidate = None::<&str>;
    let mut in_version_table = false;
    let mut selected_version = false;
    let mut selected_candidate_is_approved = false;
    let mut selected_candidate_has_foreign_origin = false;
    for line in policy.lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("Candidate:") {
            if candidate.is_some() {
                return false;
            }
            candidate = Some(value.trim());
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
        if let Some((_version, priority)) =
            fields.get(version_index).zip(fields.get(version_index + 1))
            && (*priority == "-" || priority.parse::<i32>().is_ok())
        {
            selected_version = fields
                .get(version_index)
                .is_some_and(|version| *version == expected_version);
            continue;
        }
        if selected_version && fields.len() >= 2 && fields[0].parse::<i32>().is_ok() {
            let installed_status = fields[1] == "/var/lib/dpkg/status";
            selected_candidate_is_approved |= super::is_base_origin(&fields, recipe);
            selected_candidate_has_foreign_origin |=
                !super::is_base_origin(&fields, recipe) && !installed_status;
        }
    }
    candidate == Some(expected_version)
        && in_version_table
        && selected_candidate_is_approved
        && !selected_candidate_has_foreign_origin
}
