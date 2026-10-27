use super::arguments::TrustedArgument;

mod rootless;
#[cfg(test)]
mod tests;
pub(super) use rootless::{
    trusted_find_arguments, trusted_remove_stage_arguments, trusted_rpm_query_arguments,
    trusted_rpmdb_arguments, trusted_rpmkeys_arguments, trusted_sha256sum_arguments,
    trusted_stage_mkdir_arguments,
};

const MAX_ROOTLESS_RPM_COUNT: usize = 192;

pub(super) fn trusted_arguments(arguments: &[TrustedArgument]) -> bool {
    if matches!(
        arguments,
        [
            TrustedArgument::DnfCachedir,
            TrustedArgument::DnfSystemCachedir,
            TrustedArgument::DnfVersion
        ]
    ) {
        return true;
    }
    let common = [
        "fixed:dnf-quiet",
        "fixed:dnf-release-version-45",
        "fixed:dnf-force-architecture-x86_64",
        "fixed:dnf-repository-directory",
        "fixed:dnf-cachedir",
        "fixed:dnf-system-cachedir",
        "fixed:dnf-disable-plugins",
        "fixed:dnf-gpg-check",
        "fixed:dnf-ssl-verify",
        "fixed:dnf-ssl-cacert-empty",
        "fixed:dnf-proxy-ssl-verify",
        "fixed:dnf-proxy-ssl-cacert-empty",
        "fixed:dnf-skip-if-unavailable-false",
        "fixed:dnf-fedora-gpg-check",
        "fixed:dnf-fedora-ssl-verify",
        "fixed:dnf-fedora-ssl-cacert-empty",
        "fixed:dnf-fedora-proxy-ssl-verify",
        "fixed:dnf-fedora-proxy-ssl-cacert-empty",
        "fixed:dnf-fedora-skip-if-unavailable-false",
        "fixed:dnf-updates-gpg-check",
        "fixed:dnf-updates-ssl-verify",
        "fixed:dnf-updates-ssl-cacert-empty",
        "fixed:dnf-updates-proxy-ssl-verify",
        "fixed:dnf-updates-proxy-ssl-cacert-empty",
        "fixed:dnf-updates-skip-if-unavailable-false",
        "fixed:dnf-docker-gpg-check",
        "fixed:dnf-docker-ssl-verify",
        "fixed:dnf-docker-ssl-cacert-empty",
        "fixed:dnf-docker-proxy-ssl-verify",
        "fixed:dnf-docker-proxy-ssl-cacert-empty",
        "fixed:dnf-docker-skip-if-unavailable-false",
        "fixed:dnf-disable-all-repositories",
        "fixed:dnf-enable-docker-repository",
        "fixed:dnf-enable-fedora-repository",
        "fixed:dnf-enable-updates-repository",
    ];
    let values = arguments
        .iter()
        .map(TrustedArgument::binding_value)
        .collect::<Vec<_>>();
    let suffixes: [&[&str]; 2] = [
        &["fixed:dnf-make-cache", "fixed:dnf-refresh"],
        &[
            "fixed:dnf-cache-only",
            "fixed:dnf-repoquery",
            "fixed:dnf-available",
            "fixed:dnf-latest-limit-one",
            "fixed:dnf-query-format-name-repository-id",
            "fixed:dnf-docker-ce",
            "fixed:dnf-docker-ce-cli",
            "fixed:dnf-containerd-io",
            "fixed:dnf-docker-ce-rootless-extras",
            "fixed:dnf-shadow-utils",
            "fixed:dnf-slirp4netns",
        ],
    ];
    let fixed_operation = suffixes.iter().any(|suffix| {
        let expected_len = common.len() + suffix.len();
        values.len() == expected_len
            && values
                .iter()
                .map(String::as_str)
                .eq(common.into_iter().chain(suffix.iter().copied()))
    });
    fixed_operation
        || trusted_download_only(arguments, &values, &common)
        || trusted_offline_install(arguments, &values, &common)
}

fn trusted_download_only(
    arguments: &[TrustedArgument],
    values: &[String],
    common: &[&str],
) -> bool {
    const PACKAGE_GPG_OVERRIDES: [&str; 4] = [
        "fixed:dnf-pkg-gpg-check-false",
        "fixed:dnf-fedora-pkg-gpg-check-false",
        "fixed:dnf-updates-pkg-gpg-check-false",
        "fixed:dnf-docker-pkg-gpg-check-false",
    ];
    const TRANSACTION_GUARDS: [&str; 1] = ["fixed:dnf-obsoletes-false"];
    const FIXED_SUFFIX: [&str; 11] = [
        "fixed:dnf-install",
        "fixed:dnf-downloadonly",
        "fixed:dnf-no-allow-downgrade",
        "fixed:dnf-assume-yes",
        "fixed:dnf-best",
        "fixed:dnf-no-weak-dependencies",
        "fixed:dnf-docker-ce",
        "fixed:dnf-docker-ce-cli",
        "fixed:dnf-containerd-io",
        "fixed:dnf-docker-ce-rootless-extras",
        "fixed:dnf-shadow-utils",
    ];
    let overrides_start = common.len() + 1;
    let guards_start = overrides_start + PACKAGE_GPG_OVERRIDES.len();
    let suffix_start = guards_start + TRANSACTION_GUARDS.len();
    values.len()
        == common.len()
            + 1
            + PACKAGE_GPG_OVERRIDES.len()
            + TRANSACTION_GUARDS.len()
            + FIXED_SUFFIX.len()
            + 1
        && values[..common.len()]
            .iter()
            .map(String::as_str)
            .eq(common.iter().copied())
        && matches!(
            arguments.get(common.len()),
            Some(TrustedArgument::DnfDestDir(_))
        )
        && values[overrides_start..suffix_start]
            .iter()
            .map(String::as_str)
            .take(PACKAGE_GPG_OVERRIDES.len())
            .eq(PACKAGE_GPG_OVERRIDES)
        && values[guards_start..suffix_start]
            .iter()
            .map(String::as_str)
            .eq(TRANSACTION_GUARDS)
        && values[suffix_start..suffix_start + FIXED_SUFFIX.len()]
            .iter()
            .map(String::as_str)
            .eq(FIXED_SUFFIX)
        && values
            .last()
            .is_some_and(|value| value == "fixed:dnf-slirp4netns")
}

fn trusted_offline_install(
    arguments: &[TrustedArgument],
    values: &[String],
    common: &[&str],
) -> bool {
    const PREFIX_HEAD: [&str; 6] = [
        "fixed:dnf-cache-only",
        "fixed:dnf-pkg-gpg-check-false",
        "fixed:dnf-fedora-pkg-gpg-check-false",
        "fixed:dnf-updates-pkg-gpg-check-false",
        "fixed:dnf-docker-pkg-gpg-check-false",
        "fixed:dnf-localpkg-gpgcheck-false",
    ];
    const PREFIX_TAIL: [&str; 7] = [
        "fixed:dnf-installonly-limit-unlimited",
        "fixed:dnf-obsoletes-false",
        "fixed:dnf-install",
        "fixed:dnf-no-allow-downgrade",
        "fixed:dnf-assume-yes",
        "fixed:dnf-best",
        "fixed:dnf-no-weak-dependencies",
    ];
    let offline_common = &common[..32];
    let names_position = offline_common.len() + PREFIX_HEAD.len();
    let package_start = names_position + 1 + PREFIX_TAIL.len();
    values.len() > package_start
        && values.len() <= package_start + MAX_ROOTLESS_RPM_COUNT
        && offline_common_matches(arguments, values, offline_common)
        && values[offline_common.len()..names_position]
            .iter()
            .map(String::as_str)
            .eq(PREFIX_HEAD)
        && matches!(arguments.get(names_position), Some(TrustedArgument::DnfInstallOnlyPackages(_)))
        && values[names_position + 1..package_start]
            .iter()
            .map(String::as_str)
            .eq(PREFIX_TAIL)
        && match (arguments.get(names_position), arguments.get(package_start)) {
            (
                Some(TrustedArgument::DnfInstallOnlyPackages(names)),
                Some(TrustedArgument::DnfVerifiedRpm(first_package)),
            ) => {
                names.stage() == first_package.stage()
                    && names.as_slice().len() == arguments.len() - package_start
                    && matches!(arguments.get(4), Some(TrustedArgument::DnfStageCachedir(stage)) if stage == names.stage())
                    && matches!(arguments.get(5), Some(TrustedArgument::DnfStageSystemCachedir(stage)) if stage == names.stage())
                    && arguments[package_start..].iter().all(|argument| {
                        matches!(argument, TrustedArgument::DnfVerifiedRpm(package) if package.stage() == names.stage())
                    })
            }
            _ => false,
        }
}

fn offline_common_matches(
    arguments: &[TrustedArgument],
    values: &[String],
    common: &[&str],
) -> bool {
    values.len() >= 32
        && values[..4]
            .iter()
            .map(String::as_str)
            .eq(common[..4].iter().copied())
        && matches!(arguments.get(4), Some(TrustedArgument::DnfStageCachedir(_)))
        && matches!(
            arguments.get(5),
            Some(TrustedArgument::DnfStageSystemCachedir(_))
        )
        && values[6..32]
            .iter()
            .map(String::as_str)
            .eq(common[6..32].iter().copied())
}
