use super::*;

pub(super) fn parse_catalog_report(
    text: &str,
    manifest: Option<&crate::harness::prompt_catalog::PromptManifest>,
) -> anyhow::Result<Report> {
    let manifest = manifest.ok_or_else(|| {
        anyhow::anyhow!("implementation response has no versioned prompt manifest")
    })?;
    let role = crate::harness::prompt_catalog::validate_prompt_manifest(manifest)
        .map_err(anyhow::Error::msg)?;
    anyhow::ensure!(
        matches!(
            role,
            crate::harness::prompt_catalog::PromptRole::Implementer
                | crate::harness::prompt_catalog::PromptRole::RepairWorker
        ),
        "implementation request used an unexpected prompt role"
    );
    let output = crate::harness::prompt_catalog::validate_output_with_manifest(
        manifest,
        text,
        &Default::default(),
    )
    .map_err(anyhow::Error::msg)?;
    let crate::harness::prompt_catalog::RoleOutputV1::WorkResult(report) = output else {
        anyhow::bail!("implementation returned the wrong output schema")
    };
    let json = serde_json::to_string(&report)?;
    crate::core::implementation::parse_report(&json)
}
