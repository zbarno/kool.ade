use super::*;

#[test]
fn only_ci_validated_linux_matrix_is_marked_experimental() {
    assert_eq!(
        native_support(
            HostKind::Linux,
            Architecture::Amd64,
            Some(Distribution::Ubuntu),
            Some(DistributionVersion {
                major: 24,
                minor: 4
            }),
        ),
        SupportClaim::Experimental
    );
    for distribution in [None, Some(Distribution::Debian), Some(Distribution::PopOs)] {
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
