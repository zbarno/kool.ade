use std::{collections::HashSet, path::Path};

use super::{read_optional_root_file, read_required_root_file};

const REPOSITORIES_DIRS: [&str; 2] = ["/etc/yum.repos.d", "/usr/share/dnf5/repos.d"];
const FEDORA_KEY_CONFIG: &str =
    "file:///usr/share/pki/rpm-gpg/RPM-GPG-KEY-fedora-$releasever-$basearch";
const FEDORA_METALINK: &str =
    "https://mirrors.fedoraproject.org/metalink?repo=fedora-$releasever&arch=$basearch";
const UPDATES_METALINK: &str =
    "https://mirrors.fedoraproject.org/metalink?repo=updates-released-f$releasever&arch=$basearch";

pub(super) fn inspect_existing_repositories() -> anyhow::Result<()> {
    let mut seen_ids = HashSet::new();
    let mut has_fedora = false;
    let mut has_updates = false;
    let dnf_configuration = Path::new("/etc/dnf/dnf.conf");
    if let Some(contents) = read_optional_root_file(dnf_configuration)? {
        inspect_repo_contents(
            dnf_configuration,
            &contents,
            &mut seen_ids,
            &mut has_fedora,
            &mut has_updates,
        )?;
    }
    for directory in REPOSITORIES_DIRS {
        for entry in std::fs::read_dir(directory)? {
            let path = entry?.path();
            if path.extension().is_none_or(|extension| extension != "repo") {
                continue;
            }
            let contents = read_required_root_file(&path)?;
            inspect_repo_contents(
                &path,
                &contents,
                &mut seen_ids,
                &mut has_fedora,
                &mut has_updates,
            )?;
        }
    }
    anyhow::ensure!(
        has_fedora && has_updates,
        "official Fedora 45 fedora and updates repositories are not configured"
    );
    Ok(())
}

pub(super) fn inspect_repo_contents(
    path: &Path,
    contents: &[u8],
    seen_ids: &mut HashSet<String>,
    has_fedora: &mut bool,
    has_updates: &mut bool,
) -> anyhow::Result<()> {
    let is_managed = path == Path::new(super::DOCKER_REPO_PATH);
    let text = std::str::from_utf8(contents)?;
    inspect_main_configuration_text(text)?;
    let mut current_id = None;
    let mut current_options = FedoraRepositoryOptions::default();
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if let Some(id) = line
            .strip_prefix('[')
            .and_then(|value| value.strip_suffix(']'))
        {
            validate_fedora_repository(current_id, &current_options)?;
            let id = id.trim();
            anyhow::ensure!(!id.is_empty(), "DNF repository has an empty ID");
            if matches!(id, "fedora" | "updates" | "koolade-docker-ce-stable") {
                anyhow::ensure!(
                    seen_ids.insert(id.to_owned()),
                    "DNF repository ID is defined more than once"
                );
                if id == "koolade-docker-ce-stable" {
                    anyhow::ensure!(
                        is_managed,
                        "Docker Fedora repository ID conflicts with another repository file"
                    );
                }
                *has_fedora |= id == "fedora";
                *has_updates |= id == "updates";
            }
            current_id = Some(id);
            current_options = FedoraRepositoryOptions::default();
            continue;
        }
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if current_id.is_some_and(|id| matches!(id, "fedora" | "updates"))
            && let Some((key, value)) = line.split_once('=')
        {
            let key = key.trim().to_ascii_lowercase();
            let value = value.trim();
            match key.as_str() {
                "baseurl" | "metalink" | "mirrorlist" => {
                    current_options.locators.push((key, value.to_owned()));
                }
                "gpgkey" => current_options.gpgkeys.push(value.to_owned()),
                "gpgcheck" => current_options.gpgchecks.push(value.to_owned()),
                "sslverify" | "proxy_sslverify" => {
                    current_options.tls_checks.push((key, value.to_owned()));
                }
                "sslcacert" | "proxy_sslcacert" => {
                    current_options
                        .tls_authorities
                        .push((key, value.to_owned()));
                }
                "skip_if_unavailable" => {
                    current_options.skip_if_unavailable.push(value.to_owned());
                }
                _ => {}
            }
        }
        if let Some((key, value)) = line.split_once('=')
            && matches!(key.trim(), "baseurl" | "metalink" | "mirrorlist")
            && value.to_ascii_lowercase().contains("download.docker.com")
        {
            anyhow::ensure!(
                is_managed && current_id == Some("koolade-docker-ce-stable"),
                "Docker Fedora package origin conflicts with another repository file"
            );
        }
        let signature_checks_disabled = line.split_once('=').is_some_and(|(key, value)| {
            key.trim().eq_ignore_ascii_case("gpgcheck")
                && !matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "yes" | "true"
                )
        });
        if current_id.is_some_and(|id| matches!(id, "fedora" | "updates"))
            && signature_checks_disabled
        {
            anyhow::bail!("Fedora system repository disables package signature checks");
        }
        let package_signature_check_disabled = line.split_once('=').is_some_and(|(key, value)| {
            key.trim().eq_ignore_ascii_case("pkg_gpgcheck") && !enabled_value(value)
        });
        if current_id
            .is_some_and(|id| matches!(id, "fedora" | "updates" | "koolade-docker-ce-stable"))
            && package_signature_check_disabled
        {
            anyhow::bail!("DNF candidate repository disables RPM package signature checks");
        }
    }
    validate_fedora_repository(current_id, &current_options)?;
    if is_managed {
        anyhow::ensure!(
            contents == super::DOCKER_DNF_REPO,
            "Docker Fedora repository path already contains different administrator data"
        );
    }
    Ok(())
}

pub(super) fn inspect_main_configuration(contents: &[u8]) -> anyhow::Result<()> {
    inspect_main_configuration_text(std::str::from_utf8(contents)?)
}

fn inspect_main_configuration_text(text: &str) -> anyhow::Result<()> {
    let mut in_main = false;
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if let Some(id) = line
            .strip_prefix('[')
            .and_then(|value| value.strip_suffix(']'))
        {
            in_main = id.trim() == "main";
            continue;
        }
        if !in_main || line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim().to_ascii_lowercase();
            match key.as_str() {
                "baseurl" | "metalink" | "mirrorlist" | "gpgkey" => {
                    anyhow::bail!(
                        "global DNF configuration may not override pinned Fedora origins"
                    );
                }
                "sslverify" | "proxy_sslverify" => anyhow::ensure!(
                    enabled_value(value),
                    "global DNF configuration may not disable TLS certificate verification"
                ),
                "pkg_gpgcheck" => anyhow::ensure!(
                    enabled_value(value),
                    "global DNF configuration may not disable RPM package signature checks"
                ),
                "cachedir" | "system_cachedir" => anyhow::ensure!(
                    value.trim() == "/var/cache/libdnf5",
                    "global DNF configuration may not redirect package metadata caches"
                ),
                "sslcacert" | "proxy_sslcacert" => anyhow::ensure!(
                    value.trim().is_empty(),
                    "global DNF configuration may not replace the system TLS trust store"
                ),
                "skip_if_unavailable" => anyhow::ensure!(
                    disabled_value(value),
                    "global DNF configuration may not accept stale repository metadata"
                ),
                _ => {}
            }
        }
    }
    Ok(())
}

#[derive(Default)]
struct FedoraRepositoryOptions {
    locators: Vec<(String, String)>,
    gpgkeys: Vec<String>,
    gpgchecks: Vec<String>,
    tls_checks: Vec<(String, String)>,
    tls_authorities: Vec<(String, String)>,
    skip_if_unavailable: Vec<String>,
}

fn validate_fedora_repository(
    id: Option<&str>,
    options: &FedoraRepositoryOptions,
) -> anyhow::Result<()> {
    let expected = match id {
        Some("fedora") => FEDORA_METALINK,
        Some("updates") => UPDATES_METALINK,
        _ => return Ok(()),
    };
    anyhow::ensure!(
        options.locators.len() == 1
            && options.locators[0].0 == "metalink"
            && options.locators[0].1 == expected,
        "Fedora 45 repository must use only its official metalink"
    );
    anyhow::ensure!(
        options.gpgkeys.len() == 1 && options.gpgkeys[0] == FEDORA_KEY_CONFIG,
        "Fedora 45 repository must use only its pinned signing-key path"
    );
    anyhow::ensure!(
        options.gpgchecks.len() == 1 && options.gpgchecks[0] == "1",
        "Fedora 45 repository must enable package signature checks"
    );
    anyhow::ensure!(
        options
            .tls_checks
            .iter()
            .all(|(_, value)| enabled_value(value)),
        "Fedora 45 repositories must verify TLS certificates"
    );
    anyhow::ensure!(
        options
            .tls_authorities
            .iter()
            .all(|(_, value)| value.is_empty()),
        "Fedora 45 repositories must use the system TLS trust store"
    );
    anyhow::ensure!(
        options
            .skip_if_unavailable
            .iter()
            .all(|value| disabled_value(value)),
        "Fedora 45 repositories may not continue with stale metadata"
    );
    Ok(())
}

fn enabled_value(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "yes" | "true"
    )
}

fn disabled_value(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "0" | "no" | "false"
    )
}
