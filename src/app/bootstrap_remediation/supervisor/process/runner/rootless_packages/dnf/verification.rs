use crate::app::bootstrap_remediation::{
    DnfPackageNames, DnfStageId, HashBoundDnfRpm, ProcessOutput, TrustedArgument, VerifiedDnfRpm,
    parse_installed_package_names, trusted_assets,
};

pub(super) fn rpmdb_init_arguments(stage: &DnfStageId) -> Vec<TrustedArgument> {
    vec![
        TrustedArgument::RpmDbPath(stage.clone()),
        TrustedArgument::RpmDefineMacro,
        TrustedArgument::RpmKeyringRpmdb,
        TrustedArgument::RpmInitDb,
    ]
}

pub(super) fn rpm_import_arguments(stage: &DnfStageId) -> Vec<TrustedArgument> {
    vec![
        TrustedArgument::RpmDbPath(stage.clone()),
        TrustedArgument::RpmDefineMacro,
        TrustedArgument::RpmKeyringRpmdb,
        TrustedArgument::RpmImportKeys,
        TrustedArgument::RpmDockerKey,
        TrustedArgument::RpmFedoraKey,
    ]
}

pub(super) fn rpm_list_arguments(stage: &DnfStageId) -> Vec<TrustedArgument> {
    vec![
        TrustedArgument::RpmDbPath(stage.clone()),
        TrustedArgument::RpmDefineMacro,
        TrustedArgument::RpmKeyringRpmdb,
        TrustedArgument::RpmListKeys,
    ]
}

pub(super) fn rpm_check_arguments(
    stage: &DnfStageId,
    packages: &[HashBoundDnfRpm],
) -> Vec<TrustedArgument> {
    let mut arguments = vec![
        TrustedArgument::RpmDbPath(stage.clone()),
        TrustedArgument::RpmDefineMacro,
        TrustedArgument::RpmKeyringRpmdb,
        TrustedArgument::RpmDefineMacro,
        TrustedArgument::RpmVerifyLevelAll,
        TrustedArgument::RpmDefineMacro,
        TrustedArgument::RpmVerifyFlagsZero,
        TrustedArgument::RpmCheckSignature,
        TrustedArgument::RpmVerbose,
    ];
    arguments.extend(
        packages
            .iter()
            .cloned()
            .map(TrustedArgument::DnfHashBoundRpm),
    );
    arguments
}

pub(super) fn staged_package_name_arguments(packages: &[VerifiedDnfRpm]) -> Vec<TrustedArgument> {
    let mut arguments = vec![
        TrustedArgument::RpmQuery,
        TrustedArgument::RpmQueryPackage,
        TrustedArgument::RpmQueryFormatName,
    ];
    arguments.extend(
        packages
            .iter()
            .cloned()
            .map(TrustedArgument::DnfVerifiedRpm),
    );
    arguments
}

pub(super) fn installed_package_name_arguments() -> Vec<TrustedArgument> {
    vec![
        TrustedArgument::RpmQuery,
        TrustedArgument::RpmQueryAll,
        TrustedArgument::RpmQueryFormatName,
    ]
}

pub(super) fn validate_key_inventory(output: &ProcessOutput) -> anyhow::Result<()> {
    anyhow::ensure!(
        output.exit_code == 0 && !output.stdout_truncated && !output.stderr_truncated,
        "isolated RPM key inventory did not complete with bounded output"
    );
    let mut fingerprints = Vec::new();
    for line in output.stdout.lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        anyhow::ensure!(
            fields.len() >= 4 && fields[fields.len() - 2..] == ["public", "key"],
            "isolated RPM key inventory output is malformed"
        );
        let fingerprint = fields[0].to_ascii_uppercase();
        anyhow::ensure!(
            fingerprint.len() == 40 && fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "isolated RPM key fingerprint is malformed"
        );
        fingerprints.push(fingerprint);
    }
    fingerprints.sort_unstable();
    let mut expected = [
        trusted_assets::DOCKER_DNF_KEY_FINGERPRINT.to_owned(),
        trusted_assets::FEDORA_DNF_KEY_FINGERPRINT.to_owned(),
    ];
    expected.sort_unstable();
    anyhow::ensure!(
        fingerprints == expected,
        "isolated RPM database does not contain exactly the pinned Docker and Fedora keys"
    );
    Ok(())
}

pub(super) fn staged_package_names(
    output: &ProcessOutput,
    stage: &DnfStageId,
    expected_count: usize,
) -> anyhow::Result<DnfPackageNames> {
    anyhow::ensure!(
        output.exit_code == 0 && !output.stdout_truncated && !output.stderr_truncated,
        "RPM package-name query did not complete with bounded output"
    );
    DnfPackageNames::from_rpm_query(stage.clone(), &output.stdout, expected_count)
}

pub(super) fn installed_package_names(
    output: &ProcessOutput,
) -> anyhow::Result<std::collections::HashSet<String>> {
    anyhow::ensure!(
        output.exit_code == 0 && !output.stdout_truncated && !output.stderr_truncated,
        "installed RPM name query did not complete with bounded output"
    );
    parse_installed_package_names(&output.stdout)
}

pub(super) fn ensure_no_installed_package_overlap(
    staged: &DnfPackageNames,
    installed: &std::collections::HashSet<String>,
) -> anyhow::Result<()> {
    let overlap = staged
        .as_slice()
        .iter()
        .filter(|name| installed.contains(name.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    anyhow::ensure!(
        overlap.is_empty(),
        "refusing the rootless DNF install because it would replace installed packages"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isolated_key_inventory_requires_exact_pinned_fingerprints() {
        let valid = ProcessOutput {
            exit_code: 0,
            stdout: format!(
                "{} docker Fedora Docker signing key public key\n{} fedora Fedora 45 signing key public key\n",
                trusted_assets::DOCKER_DNF_KEY_FINGERPRINT,
                trusted_assets::FEDORA_DNF_KEY_FINGERPRINT
            ),
            stderr: String::new(),
            stdout_truncated: false,
            stderr_truncated: false,
        };
        validate_key_inventory(&valid).unwrap();

        let extra = ProcessOutput {
            stdout: format!("{}\n", "A".repeat(40)),
            ..valid
        };
        assert!(validate_key_inventory(&extra).is_err());
    }

    #[test]
    fn rpm_verification_arguments_pin_keyring_and_full_verification_policy() {
        let stage = DnfStageId::new();
        let package = HashBoundDnfRpm::new(
            stage.package("docker-ce-1.0-1.x86_64.rpm").unwrap(),
            "a".repeat(64),
        )
        .unwrap();
        let arguments = rpm_check_arguments(&stage, &[package])
            .iter()
            .map(|argument| argument.as_os_string().to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert!(arguments.iter().any(|argument| argument == "--define"));
        assert!(
            arguments
                .iter()
                .any(|argument| argument == "_keyring rpmdb")
        );
        assert!(
            arguments
                .iter()
                .any(|argument| argument == "_pkgverify_level all")
        );
        assert!(
            arguments
                .iter()
                .any(|argument| argument == "_pkgverify_flags 0x0")
        );
        assert!(arguments.iter().any(|argument| argument == "--checksig"));
    }

    #[test]
    fn installed_package_overlap_fails_closed_before_transaction() {
        let stage = DnfStageId::new();
        let staged =
            DnfPackageNames::from_rpm_query(stage, "docker-ce\ncontainerd.io\n", 2).unwrap();
        let installed = ["docker-ce".to_owned()].into_iter().collect();

        assert!(ensure_no_installed_package_overlap(&staged, &installed).is_err());
        assert!(ensure_no_installed_package_overlap(&staged, &Default::default()).is_ok());
    }
}
