pub(super) fn finding_conflicts(domain: &str, finding_id: &str) -> bool {
    match domain {
        "cli:install" => finding_id == "setup:cli:install",
        "cli:scan" => finding_id == "setup:cli:scan",
        "cli:select" => finding_id == "setup:cli:select",
        "git:executable" => finding_id == "setup:git:install",
        "git:identity" | "git" => matches!(
            finding_id,
            "setup:git:install" | "setup:git:identity" | "setup:git:verify"
        ),
        "docker" => docker_engine_finding(finding_id),
        _ if domain.starts_with("cli:") => {
            let provider = domain.strip_prefix("cli:").unwrap_or_default();
            super::super::ProviderIdV1::from_probe_id(provider).is_some()
                && finding_id.starts_with(&format!("setup:cli:{provider}:"))
        }
        _ if domain.starts_with("images:") => {
            let parts = domain.split(':').collect::<Vec<_>>();
            parts.len() == 3
                && (docker_engine_finding(finding_id)
                    || (parts[1] == "general" && docker_image_finding(finding_id))
                    || finding_id == format!("setup:images:{}:{}", parts[1], parts[2])
                    || (parts[1] == "general" && finding_id == "setup:images:general:unverified"))
        }
        _ => false,
    }
}

fn docker_engine_finding(finding_id: &str) -> bool {
    let Some(code) = finding_id.strip_prefix("setup:docker:") else {
        return false;
    };
    !matches!(
        code,
        "profile" | "image-unverified" | "image-digest" | "image-architecture"
    )
}

fn docker_image_finding(finding_id: &str) -> bool {
    matches!(
        finding_id,
        "setup:docker:image-unverified"
            | "setup:docker:image-digest"
            | "setup:docker:image-architecture"
    )
}
