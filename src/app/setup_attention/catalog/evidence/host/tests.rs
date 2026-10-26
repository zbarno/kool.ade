use super::*;

#[test]
fn only_exact_recipe_linux_matrices_are_marked_experimental() {
    for (distribution, version) in [
        (Distribution::Ubuntu, (24, 4)),
        (Distribution::Debian, (13, 0)),
        (Distribution::PopOs, (24, 4)),
        (Distribution::Fedora, (45, 0)),
    ] {
        assert_eq!(
            native_support(
                HostKind::Linux,
                Architecture::Amd64,
                Some(distribution),
                Some(DistributionVersion {
                    major: version.0,
                    minor: version.1,
                }),
            ),
            SupportClaim::Experimental
        );
    }
    for distribution in [None, Some(Distribution::Other)] {
        assert_eq!(
            native_support(
                HostKind::Linux,
                Architecture::Amd64,
                distribution,
                Some(DistributionVersion {
                    major: 24,
                    minor: 4
                }),
            ),
            SupportClaim::Unverified
        );
    }
    for (distribution, version) in [
        (Distribution::Debian, (12, 0)),
        (Distribution::PopOs, (22, 4)),
        (Distribution::Fedora, (44, 0)),
    ] {
        assert_eq!(
            native_support(
                HostKind::Linux,
                Architecture::Amd64,
                Some(distribution),
                Some(DistributionVersion {
                    major: version.0,
                    minor: version.1,
                }),
            ),
            SupportClaim::Unverified
        );
    }
    assert_eq!(
        native_support(
            HostKind::Linux,
            Architecture::Arm64,
            Some(Distribution::Ubuntu),
            Some(DistributionVersion {
                major: 24,
                minor: 4
            }),
        ),
        SupportClaim::Unverified
    );
    assert_eq!(
        native_support(
            HostKind::Linux,
            Architecture::Amd64,
            Some(Distribution::Ubuntu),
            Some(DistributionVersion {
                major: 22,
                minor: 4
            }),
        ),
        SupportClaim::Unverified
    );
}

#[test]
fn rootless_mechanism_matrix_exposes_systemd_user_support_only_when_detected() {
    assert_eq!(
        rootless_mechanism(HostKind::Linux, State::Ready),
        Some(RootlessMechanism::SystemdUser)
    );
    assert_eq!(
        rootless_mechanism(HostKind::Linux, State::Unknown),
        Some(RootlessMechanism::Unknown)
    );
    assert_eq!(rootless_mechanism(HostKind::Windows, State::Unknown), None);
}
