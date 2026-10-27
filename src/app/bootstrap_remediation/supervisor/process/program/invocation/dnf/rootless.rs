use super::super::arguments::TrustedArgument;
use super::MAX_ROOTLESS_RPM_COUNT;

pub(in crate::app::bootstrap_remediation::supervisor::process::program::invocation) fn trusted_rpmdb_arguments(
    arguments: &[TrustedArgument],
) -> bool {
    matches!(
        arguments,
        [
            TrustedArgument::RpmDbPath(_),
            TrustedArgument::RpmDefineMacro,
            TrustedArgument::RpmKeyringRpmdb,
            TrustedArgument::RpmInitDb
        ]
    )
}

pub(in crate::app::bootstrap_remediation::supervisor::process::program::invocation) fn trusted_rpmkeys_arguments(
    arguments: &[TrustedArgument],
) -> bool {
    match arguments {
        [
            TrustedArgument::RpmDbPath(_),
            TrustedArgument::RpmDefineMacro,
            TrustedArgument::RpmKeyringRpmdb,
            TrustedArgument::RpmImportKeys,
            TrustedArgument::RpmDockerKey,
            TrustedArgument::RpmFedoraKey,
        ]
        | [
            TrustedArgument::RpmDbPath(_),
            TrustedArgument::RpmDefineMacro,
            TrustedArgument::RpmKeyringRpmdb,
            TrustedArgument::RpmListKeys,
        ] => true,
        [
            TrustedArgument::RpmDbPath(stage),
            TrustedArgument::RpmDefineMacro,
            TrustedArgument::RpmKeyringRpmdb,
            TrustedArgument::RpmDefineMacro,
            TrustedArgument::RpmVerifyLevelAll,
            TrustedArgument::RpmDefineMacro,
            TrustedArgument::RpmVerifyFlagsZero,
            TrustedArgument::RpmCheckSignature,
            TrustedArgument::RpmVerbose,
            packages @ ..,
        ] => {
            !packages.is_empty()
                && packages.len() <= MAX_ROOTLESS_RPM_COUNT
                && staged_rpms_match_stage(packages, stage)
        }
        _ => false,
    }
}

pub(in crate::app::bootstrap_remediation::supervisor::process::program::invocation) fn trusted_rpm_query_arguments(
    arguments: &[TrustedArgument],
) -> bool {
    match arguments {
        [
            TrustedArgument::RpmQuery,
            TrustedArgument::RpmQueryAll,
            TrustedArgument::RpmQueryFormatName,
        ] => true,
        [
            TrustedArgument::RpmQuery,
            TrustedArgument::RpmQueryPackage,
            TrustedArgument::RpmQueryFormatName,
            packages @ ..,
        ] => {
            !packages.is_empty()
                && packages.len() <= MAX_ROOTLESS_RPM_COUNT
                && all_verified_rpms_for_one_stage(packages)
        }
        _ => false,
    }
}

pub(in crate::app::bootstrap_remediation::supervisor::process::program::invocation) fn trusted_find_arguments(
    arguments: &[TrustedArgument],
) -> bool {
    matches!(
        arguments,
        [
            TrustedArgument::FindStagePackages(_),
            TrustedArgument::FindMindepth,
            TrustedArgument::FindOne,
            TrustedArgument::FindMaxdepth,
            TrustedArgument::FindOne,
            TrustedArgument::FindPrintf,
            TrustedArgument::FindRpmMetadataFormat,
        ]
    )
}

pub(in crate::app::bootstrap_remediation::supervisor::process::program::invocation) fn trusted_sha256sum_arguments(
    arguments: &[TrustedArgument],
) -> bool {
    match arguments {
        [
            TrustedArgument::Sha256sumBinary,
            TrustedArgument::EndOfOptions,
            packages @ ..,
        ] => {
            !packages.is_empty()
                && packages.len() <= MAX_ROOTLESS_RPM_COUNT
                && all_staged_rpms_for_one_stage(packages)
        }
        _ => false,
    }
}

pub(in crate::app::bootstrap_remediation::supervisor::process::program::invocation) fn trusted_remove_stage_arguments(
    arguments: &[TrustedArgument],
) -> bool {
    matches!(
        arguments,
        [
            TrustedArgument::RmRecursive,
            TrustedArgument::RmForce,
            TrustedArgument::EndOfOptions,
            TrustedArgument::DnfStageRoot(_),
        ]
    )
}

pub(in crate::app::bootstrap_remediation::supervisor::process::program::invocation) fn trusted_stage_mkdir_arguments(
    arguments: &[TrustedArgument],
) -> bool {
    matches!(
        arguments,
        [
            TrustedArgument::MkdirMode0700,
            TrustedArgument::EndOfOptions,
            TrustedArgument::DnfStageRoot(_),
            TrustedArgument::DnfStagePackages(_),
            TrustedArgument::DnfStageDatabase(_),
            TrustedArgument::DnfStageCacheDirectory(_),
        ] if same_stage(arguments)
    )
}

fn all_staged_rpms_for_one_stage(arguments: &[TrustedArgument]) -> bool {
    let Some(TrustedArgument::DnfStagedRpm(first)) = arguments.first() else {
        return false;
    };
    arguments.iter().all(|argument| {
        matches!(argument, TrustedArgument::DnfStagedRpm(package) if package.stage() == first.stage())
    })
}

fn staged_rpms_match_stage(
    arguments: &[TrustedArgument],
    stage: &super::super::arguments::DnfStageId,
) -> bool {
    !arguments.is_empty()
        && arguments.iter().all(|argument| {
            matches!(argument, TrustedArgument::DnfHashBoundRpm(package) if package.stage() == stage)
        })
}

pub(super) fn all_verified_rpms_for_one_stage(arguments: &[TrustedArgument]) -> bool {
    let Some(TrustedArgument::DnfVerifiedRpm(first)) = arguments.first() else {
        return false;
    };
    arguments.iter().all(|argument| {
        matches!(argument, TrustedArgument::DnfVerifiedRpm(package) if package.stage() == first.stage())
    })
}

fn same_stage(arguments: &[TrustedArgument]) -> bool {
    let stages = arguments
        .iter()
        .filter_map(|argument| match argument {
            TrustedArgument::DnfStageRoot(stage)
            | TrustedArgument::DnfStagePackages(stage)
            | TrustedArgument::DnfStageDatabase(stage)
            | TrustedArgument::DnfStageCacheDirectory(stage) => Some(stage),
            _ => None,
        })
        .collect::<Vec<_>>();
    stages.len() == 4 && stages.iter().all(|stage| *stage == stages[0])
}
