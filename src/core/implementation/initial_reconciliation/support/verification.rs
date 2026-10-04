pub fn verification_needs_environment(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    let Some((command_label, _)) = error.split_once(" failed:\n") else {
        return false;
    };
    let Some(command) = command_label
        .strip_prefix('`')
        .and_then(|label| label.strip_suffix('`'))
    else {
        return false;
    };
    let package_manager = package_acquisition_manager(command);
    let executable_missing = ["npm", "npx", "node", "dotnet", "cargo", "rustc"]
        .iter()
        .any(|name| {
            error.contains(&format!("{name}: command not found"))
                || error.contains(&format!("{name}: not found"))
                || error.contains(&format!("{name} is not installed"))
        });
    if executable_missing {
        return true;
    }
    let Some(package_manager) = package_manager else {
        return false;
    };
    let markers = match package_manager {
        "npm" => [
            "enotcached",
            "enetunreach",
            "eai_again",
            "econnrefused",
            "econnreset",
            "etimedout",
            "esockettimedout",
            "enotfound",
            "could not resolve host",
            "could not resolve proxy",
        ]
        .as_slice(),
        "dotnet" => ["nu1301", "nu1900", "unable to load the service index"].as_slice(),
        "cargo" => [
            "no matching package named",
            "failed to download",
            "attempting to make an http request, but --offline was specified",
            "failed to get `",
        ]
        .as_slice(),
        _ => [].as_slice(),
    };
    markers.iter().any(|marker| error.contains(marker))
}

fn package_acquisition_manager(command: &str) -> Option<&'static str> {
    for segment in command.split([';', '&', '|', '\n']) {
        let words = segment
            .split_whitespace()
            .map(|word| word.trim_matches(['\'', '"', '(', ')']))
            .collect::<Vec<_>>();
        for pair in words.windows(2) {
            let executable = pair[0].rsplit('/').next().unwrap_or_default();
            let operation = pair[1].trim_end_matches([';', '&', '|']);
            match executable {
                "npm" if matches!(operation, "ci" | "install" | "i" | "ping") => {
                    return Some("npm");
                }
                "dotnet" if operation == "restore" => return Some("dotnet"),
                "cargo" if matches!(operation, "build" | "check" | "test" | "fetch") => {
                    return Some("cargo");
                }
                _ => {}
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::verification_needs_environment;

    #[test]
    fn detects_unavailable_package_and_network_prerequisites() {
        for error in [
            "`npm ci` failed:\nnpm error code ENOTCACHED",
            "`cd ClientApp && npm ci` failed:\nnpm error code ENOTCACHED",
            "`cd ClientApp\nnpm ci` failed:\nnpm error code ENOTCACHED",
            "`dotnet restore` failed:\nerror NU1301: Unable to load the service index",
            "`cargo test --offline` failed:\nerror: no matching package named `demo` found in offline mode",
            "`npm ping` failed:\ngetaddrinfo ENOTFOUND registry.invalid",
            "`npm ci` failed:\n/bin/sh: npm: command not found",
            "`dotnet restore` failed:\n/bin/sh: dotnet: not found",
        ] {
            assert!(verification_needs_environment(error), "missed: {error}");
        }
    }

    #[test]
    fn leaves_project_failures_recoverable_by_the_agent() {
        for error in [
            "`cargo test --offline` failed:\nerror[E0308]: mismatched types",
            "`npm run test` failed:\ntest result: FAILED. 4 passed; 1 failed (connection refused)",
            "`npm run test` failed:\nnpm error Missing script: build",
            "`npm run test` failed:\nIntegration service connection timed out",
            "`cd ClientApp && npm run test` failed:\nnpm error code ENOTCACHED from an application integration test",
        ] {
            assert!(
                !verification_needs_environment(error),
                "misclassified: {error}"
            );
        }
    }
}
