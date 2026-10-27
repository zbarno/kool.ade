use crate::app::setup_attention::catalog::{
    ImageArchitectureV1 as Architecture, LinuxDistributionV1 as Distribution,
    LinuxDistributionVersionV1 as Version, LinuxPackageManagerV1 as PackageManager,
    LinuxRootlessRecipeV1 as Recipe,
};

use super::{ProbeRead, ProbeSource};

const MAX_ID_BYTES: usize = 64;
const MAX_VERSION_BYTES: usize = 32;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DistributionFacts {
    pub distribution: Distribution,
    pub version: ProbeRead<Version>,
}

pub(super) fn read_distribution(source: &impl ProbeSource) -> ProbeRead<DistributionFacts> {
    match source.read_text("/etc/os-release") {
        ProbeRead::Found(contents) => parse_distribution(&contents),
        ProbeRead::Missing => ProbeRead::Missing,
        ProbeRead::Inaccessible => ProbeRead::Inaccessible,
        ProbeRead::Malformed => ProbeRead::Malformed,
    }
}

pub fn parse_distribution(contents: &str) -> ProbeRead<DistributionFacts> {
    let Some(id) = os_release_value(contents, "ID") else {
        return ProbeRead::Malformed;
    };
    if id.is_empty()
        || id.len() > MAX_ID_BYTES
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    {
        return ProbeRead::Malformed;
    }
    let distribution = match id.to_ascii_lowercase().as_str() {
        "ubuntu" => Distribution::Ubuntu,
        "debian" => Distribution::Debian,
        "fedora" => Distribution::Fedora,
        "arch" => Distribution::Arch,
        "opensuse" | "opensuse-leap" | "opensuse-tumbleweed" | "sles" => Distribution::Suse,
        "alpine" => Distribution::Alpine,
        "pop" => Distribution::PopOs,
        _ => Distribution::Other,
    };
    let version = match os_release_value(contents, "VERSION_ID") {
        None | Some("") => ProbeRead::Missing,
        Some(value) => parse_version(value),
    };
    ProbeRead::Found(DistributionFacts {
        distribution,
        version,
    })
}

fn os_release_value<'a>(contents: &'a str, key: &str) -> Option<&'a str> {
    contents
        .lines()
        .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
        .map(str::trim)
        .map(|value| value.trim_matches(['"', '\'']))
}

fn parse_version(value: &str) -> ProbeRead<Version> {
    if value.len() > MAX_VERSION_BYTES {
        return ProbeRead::Malformed;
    }
    let (major, minor) = value.split_once('.').unwrap_or((value, "0"));
    if major.is_empty()
        || minor.is_empty()
        || !major.bytes().all(|byte| byte.is_ascii_digit())
        || !minor.bytes().all(|byte| byte.is_ascii_digit())
    {
        return ProbeRead::Malformed;
    }
    match (major.parse(), minor.parse()) {
        (Ok(major), Ok(minor)) => ProbeRead::Found(Version { major, minor }),
        _ => ProbeRead::Malformed,
    }
}

#[cfg(test)]
pub fn distribution_from_os_release(contents: &str) -> Option<(Distribution, Option<Version>)> {
    match parse_distribution(contents) {
        ProbeRead::Found(facts) => Some((
            facts.distribution,
            match facts.version {
                ProbeRead::Found(version) => Some(version),
                ProbeRead::Missing | ProbeRead::Inaccessible | ProbeRead::Malformed => None,
            },
        )),
        ProbeRead::Missing | ProbeRead::Inaccessible | ProbeRead::Malformed => None,
    }
}

pub fn expected_package_manager(distribution: Option<Distribution>) -> Option<PackageManager> {
    PackageManager::for_distribution(distribution?)
}

pub fn rootless_recipe(
    distribution: Option<Distribution>,
    version: Option<Version>,
    architecture: Architecture,
) -> Option<Recipe> {
    Recipe::for_host(distribution, version, architecture)
}
