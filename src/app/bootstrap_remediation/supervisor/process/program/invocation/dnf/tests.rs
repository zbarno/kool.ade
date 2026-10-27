use super::super::arguments::{DnfPackageNames, DnfStageId, HashBoundDnfRpm, VerifiedDnfRpm};
use super::*;

fn offline_arguments(name_count: usize, package_count: usize) -> Vec<TrustedArgument> {
    let stage = DnfStageId::new();
    let recipe = crate::app::setup_attention::catalog::LinuxRootlessRecipeV1::Fedora45Amd64;
    let mut arguments =
        crate::app::bootstrap_remediation::production::dnf::package_arguments(recipe).unwrap();
    arguments[4] = TrustedArgument::DnfStageCachedir(stage.clone());
    arguments[5] = TrustedArgument::DnfStageSystemCachedir(stage.clone());
    arguments.truncate(32);
    let names = DnfPackageNames::new(
        stage.clone(),
        (0..name_count).map(|index| format!("pkg{index}")).collect(),
    )
    .unwrap();
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
    arguments.extend((0..package_count).map(|index| {
        let package = stage.package(&format!("pkg-{index}.rpm")).unwrap();
        let hash_bound = HashBoundDnfRpm::new(package, "a".repeat(64)).unwrap();
        TrustedArgument::DnfVerifiedRpm(VerifiedDnfRpm::after_rpmkeys_success(hash_bound))
    }));
    arguments
}

#[test]
fn offline_install_allowlist_requires_one_installonly_name_per_verified_rpm() {
    for (name_count, package_count) in [(1, 2), (2, 1)] {
        let arguments = offline_arguments(name_count, package_count);
        assert!(!trusted_arguments(&arguments));
    }

    let arguments = offline_arguments(2, 2);
    assert!(trusted_arguments(&arguments));
}
