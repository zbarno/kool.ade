use crate::app::setup_attention::catalog::LinuxRootlessRecipeV1;

const DEBIAN_KEYRING: &str = "/usr/share/keyrings/debian-archive-keyring.gpg";
const UBUNTU_KEYRING: &str = "/usr/share/keyrings/ubuntu-archive-keyring.gpg";
const POP_KEYRING: &str = "/etc/apt/trusted.gpg.d/pop-keyring-2017-archive.gpg";
const POP_UBUNTU_KEYRING: &str = "/etc/apt/trusted.gpg.d/ubuntu-keyring-2018-archive.gpg";

pub(super) fn is_distro_source(
    uri: &str,
    suite: &str,
    components: &[&str],
    recipe: LinuxRootlessRecipeV1,
) -> bool {
    let (valid_suite, valid_component) = match recipe {
        LinuxRootlessRecipeV1::Debian13Amd64 => (
            match uri {
                "http://deb.debian.org/debian" | "https://deb.debian.org/debian" => {
                    matches!(suite, "trixie" | "trixie-updates")
                }
                "http://security.debian.org/debian-security"
                | "https://security.debian.org/debian-security" => suite == "trixie-security",
                _ => false,
            },
            components.iter().all(|component| {
                matches!(
                    *component,
                    "main" | "contrib" | "non-free" | "non-free-firmware"
                )
            }),
        ),
        LinuxRootlessRecipeV1::Ubuntu2404Amd64 => (
            match uri {
                "http://archive.ubuntu.com/ubuntu" | "https://archive.ubuntu.com/ubuntu" => {
                    matches!(suite, "noble" | "noble-updates" | "noble-backports")
                }
                "http://security.ubuntu.com/ubuntu" | "https://security.ubuntu.com/ubuntu" => {
                    suite == "noble-security"
                }
                _ => false,
            },
            components.iter().all(|component| {
                matches!(
                    *component,
                    "main" | "restricted" | "universe" | "multiverse"
                )
            }),
        ),
        LinuxRootlessRecipeV1::PopOs2404Amd64 => (
            match uri {
                "http://apt.pop-os.org/ubuntu" => matches!(
                    suite,
                    "noble" | "noble-security" | "noble-updates" | "noble-backports"
                ),
                "http://apt.pop-os.org/release" | "http://apt.pop-os.org/proprietary" => {
                    suite == "noble"
                }
                _ => false,
            },
            match uri {
                "http://apt.pop-os.org/ubuntu" => components.iter().all(|component| {
                    matches!(
                        *component,
                        "main" | "restricted" | "universe" | "multiverse"
                    )
                }),
                "http://apt.pop-os.org/release" | "http://apt.pop-os.org/proprietary" => {
                    components == ["main"]
                }
                _ => false,
            },
        ),
        LinuxRootlessRecipeV1::Fedora45Amd64 => (false, false),
    };
    valid_suite && valid_component && !components.is_empty()
}

pub(super) fn expected_keyring(uri: &str, recipe: LinuxRootlessRecipeV1) -> Option<&'static str> {
    match recipe {
        LinuxRootlessRecipeV1::Debian13Amd64
            if matches!(
                uri,
                "http://deb.debian.org/debian"
                    | "https://deb.debian.org/debian"
                    | "http://security.debian.org/debian-security"
                    | "https://security.debian.org/debian-security"
            ) =>
        {
            Some(DEBIAN_KEYRING)
        }
        LinuxRootlessRecipeV1::Ubuntu2404Amd64
            if matches!(
                uri,
                "http://archive.ubuntu.com/ubuntu"
                    | "https://archive.ubuntu.com/ubuntu"
                    | "http://security.ubuntu.com/ubuntu"
                    | "https://security.ubuntu.com/ubuntu"
            ) =>
        {
            Some(UBUNTU_KEYRING)
        }
        LinuxRootlessRecipeV1::PopOs2404Amd64 if uri == "http://apt.pop-os.org/ubuntu" => {
            Some(POP_UBUNTU_KEYRING)
        }
        LinuxRootlessRecipeV1::PopOs2404Amd64
            if matches!(
                uri,
                "http://apt.pop-os.org/release" | "http://apt.pop-os.org/proprietary"
            ) =>
        {
            Some(POP_KEYRING)
        }
        _ => None,
    }
}

pub(super) fn docker_repository(
    recipe: LinuxRootlessRecipeV1,
) -> Option<(&'static str, &'static str)> {
    match recipe {
        LinuxRootlessRecipeV1::Debian13Amd64 => {
            Some(("https://download.docker.com/linux/debian", "trixie"))
        }
        LinuxRootlessRecipeV1::Ubuntu2404Amd64 | LinuxRootlessRecipeV1::PopOs2404Amd64 => {
            Some(("https://download.docker.com/linux/ubuntu", "noble"))
        }
        LinuxRootlessRecipeV1::Fedora45Amd64 => None,
    }
}
