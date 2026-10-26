use crate::app::setup_attention::catalog::MachineSnapshot;

pub(in crate::persistence::setup_tasks) fn positive_domain_for(
    id: &str,
    snapshot: &MachineSnapshot,
) -> Option<String> {
    if id == "setup:legacy:provider" {
        return snapshot
            .checked_domains
            .iter()
            .find(|domain| domain.starts_with("cli:"))
            .cloned();
    }
    if matches!(
        id,
        "setup:docker:image-unverified"
            | "setup:docker:image-digest"
            | "setup:docker:image-architecture"
            | "setup:images:general:unverified"
    ) {
        let architecture = match snapshot.capability_matrix.observed_host_architecture {
            crate::app::setup_attention::catalog::ImageArchitectureV1::Amd64 => "amd64",
            crate::app::setup_attention::catalog::ImageArchitectureV1::Arm64 => "arm64",
            crate::app::setup_attention::catalog::ImageArchitectureV1::Unknown
            | crate::app::setup_attention::catalog::ImageArchitectureV1::Unsupported => {
                return None;
            }
        };
        let expected = format!("images:general:{architecture}");
        return snapshot
            .checked_domains
            .iter()
            .find(|domain| domain.as_str() == expected)
            .cloned();
    }
    if super::validation::desktop_docker_evidence(id, "docker:desktop")
        && snapshot
            .checked_domains
            .iter()
            .any(|domain| domain == "docker:desktop")
    {
        return Some("docker:desktop".to_owned());
    }
    let domain = super::validation::expected_evidence_domain(id)?;
    snapshot
        .checked_domains
        .iter()
        .any(|checked| checked == &domain)
        .then_some(domain)
}
