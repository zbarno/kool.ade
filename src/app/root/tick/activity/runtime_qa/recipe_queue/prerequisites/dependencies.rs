use crate::harness::PackageEcosystem;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct RequiredDependency {
    pub(super) id: String,
    pub(super) ecosystem: PackageEcosystem,
    pub(super) package: String,
}

pub(super) fn parse_required_dependencies(
    ids: &[String],
) -> Result<Vec<RequiredDependency>, String> {
    ids.iter()
        .map(|id| {
            let (ecosystem, package) = id.split_once(':').ok_or_else(|| {
                format!("target dependency {id} must use ecosystem:package syntax")
            })?;
            let ecosystem = match ecosystem {
                "system" => PackageEcosystem::System,
                "npm" => PackageEcosystem::Npm,
                "cargo" => PackageEcosystem::Cargo,
                _ => {
                    return Err(format!(
                        "target dependency ecosystem in {id} is unsupported"
                    ));
                }
            };
            let valid = match ecosystem {
                PackageEcosystem::System => {
                    crate::harness::resource_bridge::system_install_packages(
                        &format!("apt-get install -y {package}"),
                        package,
                        None,
                    )
                    .is_some()
                }
                PackageEcosystem::Npm | PackageEcosystem::Cargo => {
                    crate::harness::resource_bridge::valid_package_identity(package)
                }
                _ => false,
            };
            if !valid {
                return Err(format!(
                    "target dependency {id} has an invalid package identity"
                ));
            }
            Ok(RequiredDependency {
                id: id.clone(),
                ecosystem,
                package: package.into(),
            })
        })
        .collect()
}

fn debian_package_for_tool(tool: &str) -> Result<String, String> {
    let package = match tool {
        "node" => "nodejs",
        "python" => "python-is-python3",
        "pip" => "python3-pip",
        "go" => "golang-go",
        "java" | "javac" => "default-jdk-headless",
        "dotnet" => {
            return Err(
                "the Debian Bookworm broker has no approved package mapping for dotnet".into(),
            );
        }
        other => other,
    };
    let valid = !package.is_empty()
        && package.len() <= 128
        && package.as_bytes()[0].is_ascii_lowercase()
        && package.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'+' | b'.' | b'-')
        });
    if !valid {
        return Err(format!(
            "missing logical tool {tool} has no safe Debian package identity"
        ));
    }
    Ok(package.to_owned())
}

pub(super) fn system_packages_for_requirements(
    tools: &[String],
    dependencies: &[&RequiredDependency],
) -> Result<Vec<String>, String> {
    let mut packages = tools
        .iter()
        .map(|tool| debian_package_for_tool(tool))
        .collect::<Result<std::collections::BTreeSet<_>, _>>()?;
    for dependency in dependencies {
        if dependency.ecosystem != PackageEcosystem::System
            || crate::harness::resource_bridge::system_install_packages(
                &format!("apt-get install -y {}", dependency.package),
                &dependency.package,
                None,
            )
            .is_none()
        {
            return Err(format!(
                "runtime target dependency {} is not a valid approved Debian package",
                dependency.id
            ));
        }
        packages.insert(dependency.package.clone());
    }
    let packages = packages.into_iter().collect::<Vec<_>>();
    if packages.len() > 16 {
        return Err(
            "runtime recipe needs more than 16 system tools in one authorization request".into(),
        );
    }
    Ok(packages)
}
