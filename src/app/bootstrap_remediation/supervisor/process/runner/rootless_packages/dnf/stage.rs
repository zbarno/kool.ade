use crate::app::bootstrap_remediation::{
    DNF_MAX_STAGED_RPMS, DnfStageId, HashBoundDnfRpm, ProcessOutput, StagedRpm, TrustedArgument,
    VerifiedDnfRpm,
};

use crate::app::setup_attention::catalog::LinuxRootlessRecipeV1;

const MAX_STAGED_BYTES: u64 = 8 * 1024 * 1024 * 1024;

pub(super) fn create_directories(stage: &DnfStageId) -> Vec<TrustedArgument> {
    vec![
        TrustedArgument::MkdirMode0700,
        TrustedArgument::EndOfOptions,
        TrustedArgument::DnfStageRoot(stage.clone()),
        TrustedArgument::DnfStagePackages(stage.clone()),
        TrustedArgument::DnfStageDatabase(stage.clone()),
        TrustedArgument::DnfStageCacheDirectory(stage.clone()),
    ]
}

pub(super) fn download_arguments(
    recipe: LinuxRootlessRecipeV1,
    stage: &DnfStageId,
) -> anyhow::Result<Vec<TrustedArgument>> {
    let mut arguments =
        crate::app::bootstrap_remediation::production::dnf::package_arguments(recipe)?;
    let install_index = arguments
        .iter()
        .position(|argument| matches!(argument, TrustedArgument::DnfInstall))
        .ok_or_else(|| anyhow::anyhow!("fixed DNF install operation is missing"))?;
    let package_suffix = arguments.split_off(install_index + 1);
    let install = arguments
        .pop()
        .ok_or_else(|| anyhow::anyhow!("fixed DNF install operation is missing"))?;
    arguments.extend([
        TrustedArgument::DnfDestDir(stage.clone()),
        TrustedArgument::DnfPkgGpgCheckFalse,
        TrustedArgument::DnfFedoraPkgGpgCheckFalse,
        TrustedArgument::DnfUpdatesPkgGpgCheckFalse,
        TrustedArgument::DnfDockerPkgGpgCheckFalse,
        TrustedArgument::DnfObsoletesFalse,
        install,
        TrustedArgument::DnfDownloadOnly,
        TrustedArgument::DnfNoAllowDowngrade,
    ]);
    arguments.extend(package_suffix);
    Ok(arguments)
}

pub(super) fn find_arguments(stage: &DnfStageId) -> Vec<TrustedArgument> {
    vec![
        TrustedArgument::FindStagePackages(stage.clone()),
        TrustedArgument::FindMindepth,
        TrustedArgument::FindOne,
        TrustedArgument::FindMaxdepth,
        TrustedArgument::FindOne,
        TrustedArgument::FindPrintf,
        TrustedArgument::FindRpmMetadataFormat,
    ]
}

pub(super) fn hash_arguments(packages: &[StagedRpm]) -> Vec<TrustedArgument> {
    let mut arguments = vec![
        TrustedArgument::Sha256sumBinary,
        TrustedArgument::EndOfOptions,
    ];
    arguments.extend(packages.iter().cloned().map(TrustedArgument::DnfStagedRpm));
    arguments
}

pub(super) fn parse_sha256_output(
    output: &ProcessOutput,
    packages: &[StagedRpm],
) -> anyhow::Result<Vec<HashBoundDnfRpm>> {
    anyhow::ensure!(
        output.exit_code == 0 && !output.stdout_truncated && !output.stderr_truncated,
        "staged RPM digest query did not complete with bounded output"
    );
    let rows = output.stdout.lines().collect::<Vec<_>>();
    anyhow::ensure!(
        !packages.is_empty() && rows.len() == packages.len(),
        "staged RPM digest output does not match the package set"
    );
    rows.iter()
        .zip(packages)
        .map(|(row, package)| {
            let (digest, path) = row
                .split_once(" *")
                .ok_or_else(|| anyhow::anyhow!("staged RPM digest row is malformed"))?;
            anyhow::ensure!(
                digest.len() == 64
                    && digest
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
                "staged RPM digest is malformed"
            );
            anyhow::ensure!(
                path == package.path().to_string_lossy(),
                "staged RPM digest output names an unexpected path"
            );
            HashBoundDnfRpm::new(package.clone(), digest.to_owned())
        })
        .collect()
}

pub(super) fn offline_install_arguments(
    recipe: LinuxRootlessRecipeV1,
    stage: &DnfStageId,
    names: crate::app::bootstrap_remediation::DnfPackageNames,
    packages: &[VerifiedDnfRpm],
) -> anyhow::Result<Vec<TrustedArgument>> {
    anyhow::ensure!(
        names.stage() == stage && !packages.is_empty(),
        "offline DNF install artifacts are not bound to the active stage"
    );
    anyhow::ensure!(
        packages.len() <= DNF_MAX_STAGED_RPMS
            && packages.iter().all(|package| package.stage() == stage),
        "offline DNF install package set exceeds stage policy"
    );
    let mut arguments =
        crate::app::bootstrap_remediation::production::dnf::package_arguments(recipe)?;
    anyhow::ensure!(
        matches!(arguments.get(4), Some(TrustedArgument::DnfCachedir))
            && matches!(arguments.get(5), Some(TrustedArgument::DnfSystemCachedir)),
        "fixed DNF cache arguments are not in the approved positions"
    );
    arguments[4] = TrustedArgument::DnfStageCachedir(stage.clone());
    arguments[5] = TrustedArgument::DnfStageSystemCachedir(stage.clone());
    let install_index = arguments
        .iter()
        .position(|argument| matches!(argument, TrustedArgument::DnfInstall))
        .ok_or_else(|| anyhow::anyhow!("fixed DNF install operation is missing"))?;
    arguments.truncate(install_index);
    for expected in [
        TrustedArgument::DnfEnableUpdatesRepository,
        TrustedArgument::DnfEnableFedoraRepository,
        TrustedArgument::DnfEnableDockerRepository,
    ] {
        let Some(last) = arguments.pop() else {
            anyhow::bail!("fixed DNF repository configuration is incomplete")
        };
        anyhow::ensure!(
            std::mem::discriminant(&last) == std::mem::discriminant(&expected),
            "offline DNF install retained an unexpected repository configuration"
        );
    }
    anyhow::ensure!(
        arguments.len() == 32,
        "offline DNF common arguments differ from the approved fixed prefix"
    );
    arguments.extend([
        TrustedArgument::DnfCacheOnly,
        TrustedArgument::DnfPkgGpgCheckFalse,
        TrustedArgument::DnfFedoraPkgGpgCheckFalse,
        TrustedArgument::DnfUpdatesPkgGpgCheckFalse,
        TrustedArgument::DnfDockerPkgGpgCheckFalse,
        TrustedArgument::DnfLocalPackageGpgCheckFalse,
        TrustedArgument::DnfInstallOnlyPackages(names),
        TrustedArgument::DnfInstallOnlyLimitUnlimited,
        TrustedArgument::DnfObsoletesFalse,
        TrustedArgument::DnfInstall,
        TrustedArgument::DnfNoAllowDowngrade,
        TrustedArgument::DnfAssumeYes,
        TrustedArgument::DnfBest,
        TrustedArgument::DnfNoWeakDependencies,
    ]);
    arguments.extend(
        packages
            .iter()
            .cloned()
            .map(TrustedArgument::DnfVerifiedRpm),
    );
    Ok(arguments)
}

pub(super) fn parse_package_manifest(
    output: &str,
    stage: &DnfStageId,
) -> anyhow::Result<Vec<StagedRpm>> {
    let mut fields = output.split('\0').collect::<Vec<_>>();
    anyhow::ensure!(
        fields.pop() == Some("") && !fields.is_empty() && fields.len() % 7 == 0,
        "staged DNF package listing is malformed"
    );
    let count = fields.len() / 7;
    anyhow::ensure!(
        count <= DNF_MAX_STAGED_RPMS,
        "staged DNF package count exceeds policy"
    );
    let mut total_bytes = 0_u64;
    let mut packages = Vec::with_capacity(count);
    let mut names = std::collections::HashSet::with_capacity(count);
    for record in fields.as_slice().as_chunks::<7>().0 {
        let [basename, kind, uid, gid, mode, size, links] = *record;
        anyhow::ensure!(
            kind == "f" && uid == "0" && gid == "0" && links == "1",
            "staged RPM is not a single-link root-owned regular file"
        );
        let mode = u32::from_str_radix(mode, 8)?;
        anyhow::ensure!(
            mode <= 0o777 && mode & 0o022 == 0,
            "staged RPM permissions allow group or other writes"
        );
        let size = size.parse::<u64>()?;
        anyhow::ensure!(size > 0, "staged RPM payload is empty");
        total_bytes = total_bytes
            .checked_add(size)
            .ok_or_else(|| anyhow::anyhow!("staged RPM byte count overflowed"))?;
        anyhow::ensure!(
            total_bytes <= MAX_STAGED_BYTES,
            "staged RPM payload exceeds the package-set size limit"
        );
        let package = stage.package(basename)?;
        anyhow::ensure!(
            names.insert(package.basename().to_owned()),
            "staged DNF package listing repeats a filename"
        );
        packages.push(package);
    }
    anyhow::ensure!(!packages.is_empty(), "DNF download-only produced no RPMs");
    packages.sort_by(|left, right| left.basename().cmp(right.basename()));
    Ok(packages)
}

#[cfg(test)]
mod tests;
