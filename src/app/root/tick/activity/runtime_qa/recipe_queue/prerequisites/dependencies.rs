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

pub(super) fn system_packages_for_requirements(tools: &[String]) -> Result<Vec<String>, String> {
    let packages = tools
        .iter()
        .map(|tool| debian_package_for_tool(tool))
        .collect::<Result<std::collections::BTreeSet<_>, _>>()?;
    let packages = packages.into_iter().collect::<Vec<_>>();
    if packages.len() > 16 {
        return Err(
            "runtime recipe needs more than 16 system tools in one authorization request".into(),
        );
    }
    Ok(packages)
}
