use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostKind {
    Linux,
    MacOs,
    Windows,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinuxDistributionV1 {
    Ubuntu,
    Debian,
    Fedora,
    Arch,
    Suse,
    Alpine,
    PopOs,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct LinuxDistributionVersionV1 {
    pub major: u16,
    pub minor: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct LinuxKernelVersionV1 {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinuxPackageManagerV1 {
    AptGet,
    Dnf,
    Pacman,
    Zypper,
    Apk,
}

impl LinuxPackageManagerV1 {
    pub fn for_distribution(distribution: LinuxDistributionV1) -> Option<Self> {
        match distribution {
            LinuxDistributionV1::Ubuntu
            | LinuxDistributionV1::Debian
            | LinuxDistributionV1::PopOs => Some(Self::AptGet),
            LinuxDistributionV1::Fedora => Some(Self::Dnf),
            LinuxDistributionV1::Arch => Some(Self::Pacman),
            LinuxDistributionV1::Suse => Some(Self::Zypper),
            LinuxDistributionV1::Alpine => Some(Self::Apk),
            LinuxDistributionV1::Other => None,
        }
    }
}

/// An exact native-host recipe candidate. A candidate does not imply a
/// product support claim; VM validation is still required before installation.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinuxRootlessRecipeV1 {
    Debian13Amd64,
    Ubuntu2404Amd64,
    PopOs2404Amd64,
    Fedora45Amd64,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinuxRootlessMechanismV1 {
    SystemdUser,
    Unknown,
}
