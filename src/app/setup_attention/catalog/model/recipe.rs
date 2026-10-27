use super::identifiers::{
    ImageArchitectureV1, LinuxDistributionV1, LinuxDistributionVersionV1, LinuxRootlessRecipeV1,
};

impl LinuxRootlessRecipeV1 {
    pub(crate) fn from_os_version(os_version: &str) -> Option<Self> {
        [
            Self::Debian13Amd64,
            Self::Ubuntu2404Amd64,
            Self::PopOs2404Amd64,
            Self::Fedora45Amd64,
        ]
        .into_iter()
        .find(|recipe| {
            os_version
                .strip_prefix(recipe.os_version_prefix())
                .is_some_and(|kernel| !kernel.is_empty())
        })
    }

    pub(crate) fn for_host(
        distribution: Option<LinuxDistributionV1>,
        version: Option<LinuxDistributionVersionV1>,
        architecture: ImageArchitectureV1,
    ) -> Option<Self> {
        let version = version?;
        let distribution = distribution?;
        if architecture != ImageArchitectureV1::Amd64 {
            return None;
        }
        let recipe = match (distribution, version.major, version.minor) {
            (LinuxDistributionV1::Debian, 13, 0) => Some(Self::Debian13Amd64),
            (LinuxDistributionV1::Ubuntu, 24, 4) => Some(Self::Ubuntu2404Amd64),
            (LinuxDistributionV1::PopOs, 24, 4) => Some(Self::PopOs2404Amd64),
            (LinuxDistributionV1::Fedora, 45, 0) => Some(Self::Fedora45Amd64),
            _ => None,
        }?;
        (recipe.identity() == (distribution, version, architecture)).then_some(recipe)
    }

    pub(crate) const fn identity(
        self,
    ) -> (
        LinuxDistributionV1,
        LinuxDistributionVersionV1,
        ImageArchitectureV1,
    ) {
        match self {
            Self::Debian13Amd64 => (
                LinuxDistributionV1::Debian,
                LinuxDistributionVersionV1 {
                    major: 13,
                    minor: 0,
                },
                ImageArchitectureV1::Amd64,
            ),
            Self::Ubuntu2404Amd64 => (
                LinuxDistributionV1::Ubuntu,
                LinuxDistributionVersionV1 {
                    major: 24,
                    minor: 4,
                },
                ImageArchitectureV1::Amd64,
            ),
            Self::PopOs2404Amd64 => (
                LinuxDistributionV1::PopOs,
                LinuxDistributionVersionV1 {
                    major: 24,
                    minor: 4,
                },
                ImageArchitectureV1::Amd64,
            ),
            Self::Fedora45Amd64 => (
                LinuxDistributionV1::Fedora,
                LinuxDistributionVersionV1 {
                    major: 45,
                    minor: 0,
                },
                ImageArchitectureV1::Amd64,
            ),
        }
    }

    pub(crate) const fn os_version_prefix(self) -> &'static str {
        match self {
            Self::Debian13Amd64 => "debian-13-amd64-kernel-",
            Self::Ubuntu2404Amd64 => "ubuntu-24.04-amd64-kernel-",
            Self::PopOs2404Amd64 => "pop-24.04-amd64-kernel-",
            Self::Fedora45Amd64 => "fedora-45-amd64-kernel-",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipes_match_only_their_explicit_distribution_release_and_architecture() {
        for (distribution, version, recipe) in [
            (
                LinuxDistributionV1::Debian,
                LinuxDistributionVersionV1 {
                    major: 13,
                    minor: 0,
                },
                LinuxRootlessRecipeV1::Debian13Amd64,
            ),
            (
                LinuxDistributionV1::Ubuntu,
                LinuxDistributionVersionV1 {
                    major: 24,
                    minor: 4,
                },
                LinuxRootlessRecipeV1::Ubuntu2404Amd64,
            ),
            (
                LinuxDistributionV1::PopOs,
                LinuxDistributionVersionV1 {
                    major: 24,
                    minor: 4,
                },
                LinuxRootlessRecipeV1::PopOs2404Amd64,
            ),
            (
                LinuxDistributionV1::Fedora,
                LinuxDistributionVersionV1 {
                    major: 45,
                    minor: 0,
                },
                LinuxRootlessRecipeV1::Fedora45Amd64,
            ),
        ] {
            assert_eq!(
                LinuxRootlessRecipeV1::for_host(
                    Some(distribution),
                    Some(version),
                    ImageArchitectureV1::Amd64,
                ),
                Some(recipe)
            );
            assert_eq!(
                recipe.identity(),
                (distribution, version, ImageArchitectureV1::Amd64)
            );
        }
    }

    #[test]
    fn recipes_reject_other_releases_architectures_and_missing_identity() {
        for (distribution, version, architecture) in [
            (
                LinuxDistributionV1::Debian,
                Some(LinuxDistributionVersionV1 {
                    major: 12,
                    minor: 0,
                }),
                ImageArchitectureV1::Amd64,
            ),
            (
                LinuxDistributionV1::Ubuntu,
                Some(LinuxDistributionVersionV1 {
                    major: 24,
                    minor: 4,
                }),
                ImageArchitectureV1::Arm64,
            ),
            (
                LinuxDistributionV1::Fedora,
                Some(LinuxDistributionVersionV1 {
                    major: 44,
                    minor: 0,
                }),
                ImageArchitectureV1::Amd64,
            ),
        ] {
            assert_eq!(
                LinuxRootlessRecipeV1::for_host(Some(distribution), version, architecture),
                None
            );
        }
        assert_eq!(
            LinuxRootlessRecipeV1::for_host(
                Some(LinuxDistributionV1::Ubuntu),
                None,
                ImageArchitectureV1::Amd64,
            ),
            None
        );
    }
}
