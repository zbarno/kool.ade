use std::collections::HashMap;

use crate::app::bootstrap_remediation::TrustedArgument;

#[cfg(target_os = "linux")]
const DNF: &str = "/usr/bin/dnf5";
#[cfg(target_os = "linux")]
const MAX_QUERY_BYTES: usize = 256 * 1024;
const CANDIDATES: [(&str, &[&str]); 6] = [
    ("docker-ce", &["koolade-docker-ce-stable"]),
    ("docker-ce-cli", &["koolade-docker-ce-stable"]),
    ("containerd.io", &["koolade-docker-ce-stable"]),
    ("docker-ce-rootless-extras", &["koolade-docker-ce-stable"]),
    ("shadow-utils", &["fedora", "updates"]),
    ("slirp4netns", &["fedora", "updates"]),
];

pub(super) fn trusted_arguments() -> Vec<TrustedArgument> {
    common_arguments()
        .into_iter()
        .chain([
            TrustedArgument::DnfCacheOnly,
            TrustedArgument::DnfRepoQuery,
            TrustedArgument::DnfAvailable,
            TrustedArgument::DnfLatestLimitOne,
            TrustedArgument::DnfQueryFormatNameRepositoryId,
            TrustedArgument::DnfDockerCe,
            TrustedArgument::DnfDockerCeCli,
            TrustedArgument::DnfContainerdIo,
            TrustedArgument::DnfDockerRootlessExtras,
            TrustedArgument::DnfShadowUtils,
            TrustedArgument::DnfSlirp4netns,
        ])
        .collect()
}

pub(super) fn common_arguments() -> Vec<TrustedArgument> {
    vec![
        TrustedArgument::DnfQuiet,
        TrustedArgument::DnfReleaseVersion45,
        TrustedArgument::DnfForceArchitectureX8664,
        TrustedArgument::DnfRepositoryDirectory,
        TrustedArgument::DnfCachedir,
        TrustedArgument::DnfSystemCachedir,
        TrustedArgument::DnfDisablePlugins,
        TrustedArgument::DnfGpgCheck,
        TrustedArgument::DnfSslVerify,
        TrustedArgument::DnfSslCaCertEmpty,
        TrustedArgument::DnfProxySslVerify,
        TrustedArgument::DnfProxySslCaCertEmpty,
        TrustedArgument::DnfSkipIfUnavailableFalse,
        TrustedArgument::DnfFedoraGpgCheck,
        TrustedArgument::DnfFedoraSslVerify,
        TrustedArgument::DnfFedoraSslCaCertEmpty,
        TrustedArgument::DnfFedoraProxySslVerify,
        TrustedArgument::DnfFedoraProxySslCaCertEmpty,
        TrustedArgument::DnfFedoraSkipIfUnavailableFalse,
        TrustedArgument::DnfUpdatesGpgCheck,
        TrustedArgument::DnfUpdatesSslVerify,
        TrustedArgument::DnfUpdatesSslCaCertEmpty,
        TrustedArgument::DnfUpdatesProxySslVerify,
        TrustedArgument::DnfUpdatesProxySslCaCertEmpty,
        TrustedArgument::DnfUpdatesSkipIfUnavailableFalse,
        TrustedArgument::DnfDockerGpgCheck,
        TrustedArgument::DnfDockerSslVerify,
        TrustedArgument::DnfDockerSslCaCertEmpty,
        TrustedArgument::DnfDockerProxySslVerify,
        TrustedArgument::DnfDockerProxySslCaCertEmpty,
        TrustedArgument::DnfDockerSkipIfUnavailableFalse,
        TrustedArgument::DnfDisableAllRepositories,
        TrustedArgument::DnfEnableDockerRepository,
        TrustedArgument::DnfEnableFedoraRepository,
        TrustedArgument::DnfEnableUpdatesRepository,
    ]
}

pub(super) fn check_docker_candidates() -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    {
        verify_dnf_executable()?;
        let output = run_candidate_query()?;
        let text = std::str::from_utf8(&output)?;
        anyhow::ensure!(
            candidates_have_approved_origins(text),
            "DNF candidates are missing or come from an unapproved signed repository"
        );
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        anyhow::bail!("Fedora rootless Docker package candidates are Linux-only")
    }
}

pub(super) fn candidates_have_approved_origins(output: &str) -> bool {
    let mut found = HashMap::<&str, usize>::new();
    for line in output.lines().filter(|line| !line.is_empty()) {
        let Some((package, repo)) = line.split_once('|') else {
            return false;
        };
        if repo.contains('|') || package.contains('|') {
            return false;
        }
        let Some((_, approved_repos)) = CANDIDATES.iter().find(|(name, _)| *name == package) else {
            return false;
        };
        if !approved_repos.contains(&repo) {
            return false;
        }
        if found.insert(package, 1).is_some() {
            return false;
        }
    }
    found.len() == CANDIDATES.len()
        && CANDIDATES
            .iter()
            .all(|(package, _)| found.contains_key(package))
}

#[cfg(target_os = "linux")]
fn verify_dnf_executable() -> anyhow::Result<()> {
    use std::os::unix::fs::MetadataExt;

    let path = std::path::Path::new(DNF);
    let metadata = std::fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_file()
            && !metadata.file_type().is_symlink()
            && metadata.uid() == 0
            && metadata.mode() & 0o022 == 0
            && path.canonicalize()? == path,
        "DNF executable is not a trusted system program"
    );
    Ok(())
}

#[cfg(target_os = "linux")]
fn run_candidate_query() -> anyhow::Result<Vec<u8>> {
    use std::{
        io::Read,
        process::{Command, Stdio},
        thread,
        time::{Duration, Instant},
    };

    let arguments = trusted_arguments()
        .iter()
        .map(TrustedArgument::as_os_string)
        .collect::<Vec<_>>();
    let mut child = Command::new(DNF)
        .args(arguments)
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
        .ok_or_else(|| anyhow::anyhow!("DNF candidate output pipe is unavailable"))?;
    let reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .by_ref()
            .take((MAX_QUERY_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        match child.try_wait()? {
            Some(status) => break status,
            None if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            None => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                anyhow::bail!("DNF candidate inspection timed out")
            }
        }
    };
    let bytes = reader
        .join()
        .map_err(|_| anyhow::anyhow!("DNF candidate output reader failed"))??;
    anyhow::ensure!(
        status.success() && bytes.len() <= MAX_QUERY_BYTES,
        "DNF could not report a bounded package candidate list"
    );
    Ok(bytes)
}
