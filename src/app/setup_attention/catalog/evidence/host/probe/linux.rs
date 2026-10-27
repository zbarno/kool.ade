use crate::app::setup_attention::catalog::{
    EvidenceCode as Code, EvidenceState as State, ImageArchitectureV1 as Architecture,
    LinuxDistributionV1 as Distribution, LinuxDistributionVersionV1 as DistributionVersion,
    LinuxKernelVersionV1 as KernelVersion, LinuxPackageManagerV1 as PackageManager,
    LinuxRootlessRecipeV1 as Recipe, ProbeObservationV1 as Observation,
};

use super::{ProbeFact, ProbeRead, ProbeSource, distro, fact, state_code};

mod cgroups;
mod mappings;
mod systemd;
mod userns;

#[derive(Clone, Debug)]
pub struct LinuxProbeFacts {
    pub distribution: Option<(Distribution, Option<DistributionVersion>)>,
    pub recipe: ProbeFact,
    pub kernel: ProbeFact,
    pub user_namespaces: ProbeFact,
    pub subuid: ProbeFact,
    pub subgid: ProbeFact,
    pub helpers: ProbeFact,
    pub cgroups: ProbeFact,
    pub uid_mappings: ProbeFact,
    pub service_manager: ProbeFact,
    pub package_manager: ProbeFact,
}

impl LinuxProbeFacts {
    pub(super) fn not_applicable() -> Self {
        let not_applicable = || ProbeFact {
            state: State::NotApplicable,
            code: Code::HostCapabilityNotProbed,
            observation: None,
        };
        Self {
            distribution: None,
            recipe: not_applicable(),
            kernel: not_applicable(),
            user_namespaces: not_applicable(),
            subuid: not_applicable(),
            subgid: not_applicable(),
            helpers: not_applicable(),
            cgroups: not_applicable(),
            uid_mappings: not_applicable(),
            service_manager: not_applicable(),
            package_manager: not_applicable(),
        }
    }
}

pub fn inspect(
    source: &impl ProbeSource,
    uid: u32,
    username: Option<String>,
    architecture: Architecture,
) -> LinuxProbeFacts {
    let distribution_read = distro::read_distribution(source);
    let distribution = match &distribution_read {
        ProbeRead::Found(facts) => Some((
            facts.distribution,
            match facts.version {
                ProbeRead::Found(version) => Some(version),
                ProbeRead::Missing | ProbeRead::Inaccessible | ProbeRead::Malformed => None,
            },
        )),
        ProbeRead::Missing | ProbeRead::Inaccessible | ProbeRead::Malformed => None,
    };
    let recipe = recipe_fact(&distribution_read, architecture);
    let kernel = kernel_fact(source.read_text("/proc/sys/kernel/osrelease"));
    let user_namespaces = userns::inspect(source);
    let subuid = mappings::inspect(source.read_text("/etc/subuid"), uid, username.as_deref());
    let subgid = mappings::inspect(source.read_text("/etc/subgid"), uid, username.as_deref());
    let uid_mappings = combine_mappings(&subuid, &subgid);
    let helpers = userns::inspect_helpers(source);
    let cgroups = cgroups::inspect(source, uid);
    let service_manager = systemd::inspect(source, uid);
    let expected_manager = distro::expected_package_manager(distribution.map(|(id, _)| id));
    let candidate_recipe = match recipe.observation.as_ref() {
        Some(Observation::LinuxRootlessRecipe {
            candidate: Some(candidate),
            ..
        }) => Some(candidate),
        _ => None,
    };
    let package_manager = package_manager_fact(source, expected_manager, candidate_recipe);

    LinuxProbeFacts {
        distribution,
        recipe,
        kernel,
        user_namespaces,
        subuid,
        subgid,
        helpers,
        cgroups,
        uid_mappings,
        service_manager,
        package_manager,
    }
}

fn recipe_fact(
    distribution: &ProbeRead<distro::DistributionFacts>,
    architecture: Architecture,
) -> ProbeFact {
    let (state, candidate, distribution_state) = match distribution {
        ProbeRead::Found(facts) => {
            let version = match facts.version {
                ProbeRead::Found(version) => Some(version),
                ProbeRead::Missing | ProbeRead::Inaccessible | ProbeRead::Malformed => None,
            };
            if matches!(facts.version, ProbeRead::Malformed) {
                (State::Failed, None, State::Failed)
            } else if matches!(facts.version, ProbeRead::Inaccessible) {
                (State::Unknown, None, State::Unknown)
            } else if let Some(recipe) =
                distro::rootless_recipe(Some(facts.distribution), version, architecture)
            {
                (State::Ready, Some(recipe), State::Ready)
            } else {
                (State::Unknown, None, State::Ready)
            }
        }
        ProbeRead::Missing => (State::Missing, None, State::Missing),
        ProbeRead::Inaccessible => (State::Unknown, None, State::Unknown),
        ProbeRead::Malformed => (State::Failed, None, State::Failed),
    };
    let code = match state {
        State::Ready => Code::LinuxRootlessRecipeCandidate,
        State::Missing => Code::LinuxHostCheckMissing,
        State::Unknown => match distribution {
            ProbeRead::Inaccessible => Code::LinuxProbeInaccessible,
            ProbeRead::Found(facts) if matches!(facts.version, ProbeRead::Inaccessible) => {
                Code::LinuxProbeInaccessible
            }
            _ => Code::LinuxRootlessRecipeUnverified,
        },
        State::Failed => Code::LinuxProbeMalformed,
        _ => Code::LinuxProbeUnsupported,
    };
    fact(
        state,
        code,
        Some(Observation::LinuxRootlessRecipe {
            candidate,
            distribution_state,
        }),
    )
}

fn kernel_fact(read: ProbeRead<String>) -> ProbeFact {
    let (state, code, version) = match read {
        ProbeRead::Found(release) => match parse_kernel_version(&release) {
            Some(version) => (State::Ready, Code::LinuxHostCheckReady, Some(version)),
            None => (State::Failed, Code::LinuxProbeMalformed, None),
        },
        ProbeRead::Missing => (State::Missing, Code::LinuxHostCheckMissing, None),
        ProbeRead::Inaccessible => (State::Unknown, Code::LinuxProbeInaccessible, None),
        ProbeRead::Malformed => (State::Failed, Code::LinuxProbeMalformed, None),
    };
    fact(
        state,
        code,
        version.map(|version| Observation::LinuxKernel { version }),
    )
}

fn parse_kernel_version(release: &str) -> Option<KernelVersion> {
    let numeric = release.split(['-', '+']).next()?.trim();
    let mut parts = numeric.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    if major == 0 || parts.next().is_some() {
        return None;
    }
    Some(KernelVersion {
        major,
        minor,
        patch,
    })
}

fn package_manager_fact(
    source: &impl ProbeSource,
    expected: Option<PackageManager>,
    recipe: Option<&Recipe>,
) -> ProbeFact {
    let Some(expected) = expected else {
        return fact(
            State::Unknown,
            Code::HostCapabilityNotProbed,
            Some(Observation::LinuxPackageManager {
                expected: None,
                found: None,
            }),
        );
    };
    let read = source.executable(package_manager_binary(expected, recipe));
    let (state, code, found) = match read {
        ProbeRead::Found(true) => (
            State::Ready,
            Code::LinuxPackageManagerDetected,
            Some(expected),
        ),
        ProbeRead::Found(false) | ProbeRead::Missing => {
            (State::Missing, Code::LinuxPackageManagerMissing, None)
        }
        ProbeRead::Inaccessible => (State::Unknown, Code::LinuxProbeInaccessible, None),
        ProbeRead::Malformed => (State::Failed, Code::LinuxProbeMalformed, None),
    };
    fact(
        state,
        code,
        Some(Observation::LinuxPackageManager {
            expected: Some(expected),
            found,
        }),
    )
}

fn package_manager_binary(manager: PackageManager, recipe: Option<&Recipe>) -> &'static str {
    match manager {
        PackageManager::AptGet => "apt-get",
        PackageManager::Dnf if recipe == Some(&Recipe::Fedora45Amd64) => "dnf5",
        PackageManager::Dnf => "dnf",
        PackageManager::Pacman => "pacman",
        PackageManager::Zypper => "zypper",
        PackageManager::Apk => "apk",
    }
}

fn combine_mappings(subuid: &ProbeFact, subgid: &ProbeFact) -> ProbeFact {
    let state = if subuid.state == State::Failed || subgid.state == State::Failed {
        State::Failed
    } else if subuid.state == State::Unknown || subgid.state == State::Unknown {
        State::Unknown
    } else if subuid.state == State::Missing || subgid.state == State::Missing {
        State::Missing
    } else if subuid.state == State::Ready && subgid.state == State::Ready {
        State::Ready
    } else {
        State::Unknown
    };
    let code = match state {
        State::Ready => Code::LinuxUidMappingsConfigured,
        State::Missing => Code::LinuxUidMappingsMissing,
        State::Failed => Code::LinuxProbeMalformed,
        State::Unknown => Code::LinuxProbeInaccessible,
        _ => state_code(state),
    };
    fact(state, code, None)
}

#[cfg(test)]
pub fn parse_distribution(contents: &str) -> ProbeRead<distro::DistributionFacts> {
    distro::parse_distribution(contents)
}

#[cfg(test)]
pub fn parse_kernel(release: &str) -> Option<KernelVersion> {
    parse_kernel_version(release)
}
