use crate::app::setup_attention::catalog::{
    ImageArchitectureV1 as Architecture, LinuxDistributionV1 as Distribution,
    LinuxPackageManagerV1 as PackageManager, LinuxRootlessRecipeV1 as Recipe,
};

use super::super::distro::{expected_package_manager, rootless_recipe};
use super::super::{ProbeRead, distribution_from_os_release, parse_distribution, parse_kernel};

#[test]
fn distro_identity_version_recipe_and_package_manager_use_exact_ids() {
    let parsed = parse_distribution("ID=debian\nID_LIKE=debian\nVERSION_ID=13.2\n");
    let ProbeRead::Found(parsed) = parsed else {
        panic!("expected a valid Debian identity");
    };
    assert_eq!(parsed.distribution, Distribution::Debian);
    assert_eq!(
        parsed.version,
        ProbeRead::Found(
            crate::app::setup_attention::catalog::LinuxDistributionVersionV1 {
                major: 13,
                minor: 2,
            }
        )
    );
    assert_eq!(
        distribution_from_os_release("ID=fedora\nVERSION_ID=42\n"),
        Some((
            Distribution::Fedora,
            Some(
                crate::app::setup_attention::catalog::LinuxDistributionVersionV1 {
                    major: 42,
                    minor: 0,
                }
            ),
        ))
    );
    assert_eq!(
        expected_package_manager(Some(Distribution::Fedora)),
        Some(PackageManager::Dnf)
    );
    assert_eq!(
        expected_package_manager(Some(Distribution::PopOs)),
        Some(PackageManager::AptGet)
    );
    assert_eq!(expected_package_manager(Some(Distribution::Other)), None);
    assert_eq!(
        rootless_recipe(
            Some(Distribution::Ubuntu),
            Some(
                crate::app::setup_attention::catalog::LinuxDistributionVersionV1 {
                    major: 24,
                    minor: 4,
                }
            ),
            Architecture::Amd64,
        ),
        Some(Recipe::Ubuntu2404Amd64)
    );
    for (distribution, version, architecture, expected) in [
        (
            Distribution::Debian,
            (13, 0),
            Architecture::Amd64,
            Some(Recipe::Debian13Amd64),
        ),
        (
            Distribution::PopOs,
            (24, 4),
            Architecture::Amd64,
            Some(Recipe::PopOs2404Amd64),
        ),
        (
            Distribution::Fedora,
            (45, 0),
            Architecture::Amd64,
            Some(Recipe::Fedora45Amd64),
        ),
        (Distribution::Ubuntu, (24, 4), Architecture::Arm64, None),
        (Distribution::Ubuntu, (22, 4), Architecture::Amd64, None),
        (Distribution::Fedora, (44, 0), Architecture::Amd64, None),
    ] {
        assert_eq!(
            rootless_recipe(
                Some(distribution),
                Some(
                    crate::app::setup_attention::catalog::LinuxDistributionVersionV1 {
                        major: version.0,
                        minor: version.1,
                    }
                ),
                architecture,
            ),
            expected
        );
    }
}

#[test]
fn kernel_release_parser_keeps_numeric_version_and_rejects_malformed_values() {
    assert_eq!(
        parse_kernel("6.8.0-35-generic"),
        Some(crate::app::setup_attention::catalog::LinuxKernelVersionV1 {
            major: 6,
            minor: 8,
            patch: 0,
        })
    );
    assert_eq!(parse_kernel("not-a-kernel"), None);
    assert_eq!(parse_kernel("6.x.2"), None);
    assert_eq!(parse_kernel("0.1.2"), None);
}
