use std::time::Duration;

use crate::app::bootstrap_remediation::{TrustedArgument, TrustedInvocation, TrustedProgram};
use crate::app::bootstrap_repair_policy::Host;

#[test]
fn dnf_invocations_require_pinned_cache_paths_and_repository_operations() {
    let version = TrustedInvocation::new(
        Host::Linux,
        TrustedProgram::Dnf,
        vec![
            TrustedArgument::DnfCachedir,
            TrustedArgument::DnfSystemCachedir,
            TrustedArgument::DnfVersion,
        ],
        Duration::from_secs(20),
    )
    .unwrap();
    assert_eq!(
        version.arguments_for_test(),
        [
            "--setopt=cachedir=/var/cache/libdnf5",
            "--setopt=system_cachedir=/var/cache/libdnf5",
            "--version",
        ]
    );
    assert!(
        TrustedInvocation::new(
            Host::Linux,
            TrustedProgram::Dnf,
            vec![TrustedArgument::DnfVersion],
            Duration::from_secs(20),
        )
        .is_err()
    );
    assert_eq!(
        TrustedProgram::Dnf.resolve(Host::Linux).unwrap(),
        std::path::PathBuf::from("/usr/bin/dnf5")
    );
    assert!(
        TrustedInvocation::new(
            Host::Linux,
            TrustedProgram::Dnf,
            vec![TrustedArgument::TestLiteral("install *".into())],
            Duration::from_secs(20),
        )
        .is_err()
    );
}

#[test]
fn fedora_package_setup_preflight_pins_dnf_cache_paths() {
    let setup = TrustedInvocation::new_for_current_user_systemd(
        Host::Linux,
        TrustedProgram::LinuxRootlessPackageSetup,
        vec![TrustedArgument::LinuxRootlessRecipe(
            crate::app::setup_attention::catalog::LinuxRootlessRecipeV1::Fedora45Amd64,
        )],
        Duration::from_secs(60),
    )
    .unwrap();
    let preflight = setup.native_elevation_preflight().unwrap();
    assert_eq!(preflight.program_for_test(), TrustedProgram::Dnf);
    assert_eq!(
        preflight.arguments_for_test(),
        [
            "--setopt=cachedir=/var/cache/libdnf5",
            "--setopt=system_cachedir=/var/cache/libdnf5",
            "--version",
        ]
    );
    assert!(
        TrustedInvocation::new(
            Host::Linux,
            TrustedProgram::LinuxRootlessPackageSetup,
            vec![
                TrustedArgument::LinuxRootlessRecipe(
                    crate::app::setup_attention::catalog::LinuxRootlessRecipeV1::Fedora45Amd64,
                ),
                TrustedArgument::LinuxRootlessRecipe(
                    crate::app::setup_attention::catalog::LinuxRootlessRecipeV1::Fedora45Amd64,
                ),
            ],
            Duration::from_secs(60),
        )
        .is_err()
    );
}
