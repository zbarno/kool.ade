//! App-owned, immutable Docker build contexts for trusted runtime composites.
//!
//! No repository Dockerfile, package script, model output, host credential or
//! unverified URL is admitted to this context. Only the exact source manifests
//! from the bundled executor lock can enter the generated Dockerfile.

use std::{collections::BTreeMap, io::Cursor};

use super::{ImageLock, RuntimeProfile, SelectedImage, project_plan::CompositionCandidate};
use crate::execution::docker::image_extension::builder::GeneratedImageContext;

pub(super) fn verified_context(
    plan: &CompositionCandidate,
) -> anyhow::Result<(GeneratedImageContext, SelectedImage)> {
    let lock = ImageLock::bundled()?;
    let architecture = plan
        .platform
        .strip_prefix("linux/")
        .ok_or_else(|| anyhow::anyhow!("Composite platform is not Linux"))?;
    let sources = std::iter::once(&plan.foundation).chain(plan.components.iter());
    for source in sources {
        let approved = lock.select(source.profile, architecture)?;
        anyhow::ensure!(
            approved == *source
                && !matches!(
                    source.profile,
                    RuntimeProfile::General | RuntimeProfile::BrowserQa | RuntimeProfile::DesktopQa
                ),
            "Composite source is not an exact approved toolchain image"
        );
    }
    let canonical_plan = match lock.plan_project_runtime(architecture, &plan.required_tools)? {
        super::ProjectRuntimeImagePlan::Composition(candidate) => candidate,
        _ => anyhow::bail!("Composite plan no longer matches the approved image lock"),
    };
    anyhow::ensure!(
        canonical_plan == *plan,
        "Composite plan differs from its current immutable canonical identity"
    );
    let mut versions = BTreeMap::new();
    for source in std::iter::once(&plan.foundation).chain(plan.components.iter()) {
        for (tool, version) in &source.tool_versions {
            if !matches!(tool.as_str(), "base" | "shell") {
                anyhow::ensure!(
                    version
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.'),
                    "Approved tool version cannot appear in a bounded runtime probe"
                );
                versions.insert(tool.clone(), version.clone());
            }
        }
    }
    anyhow::ensure!(
        plan.required_tools.iter().all(|tool| {
            versions.contains_key(tool) || (tool == "python3" && versions.contains_key("python"))
        }),
        "Composite runtime cannot satisfy all requested logical tools"
    );
    let mut dockerfile = format!(
        "FROM {}\nLABEL io.koolade.content={} io.koolade.base-digest={} io.koolade.composition-policy=project-composition-v1\n",
        plan.foundation.reference, plan.recipe_sha256, plan.foundation.digest
    );
    for component in &plan.components {
        let line = match component.profile {
            RuntimeProfile::Rust => format!(
                "COPY --from={0} /usr/local/cargo/ /usr/local/cargo/\nCOPY --from={0} /usr/local/rustup/ /usr/local/rustup/\n",
                component.reference
            ),
            RuntimeProfile::Node | RuntimeProfile::Python => format!(
                "COPY --from={} /usr/local/ /usr/local/\n",
                component.reference
            ),
            RuntimeProfile::Go => format!(
                "COPY --from={} /usr/local/go/ /usr/local/go/\n",
                component.reference
            ),
            RuntimeProfile::General | RuntimeProfile::BrowserQa | RuntimeProfile::DesktopQa => {
                anyhow::bail!("Unapproved composite source profile");
            }
        };
        dockerfile.push_str(&line);
    }
    dockerfile.push_str(
        "ENV PATH=/usr/local/cargo/bin:/usr/local/go/bin:/usr/local/bin:/usr/local/sbin:/usr/bin:/usr/sbin:/bin:/sbin\n         ENV CARGO_HOME=/usr/local/cargo RUSTUP_HOME=/usr/local/rustup\n         USER 10001:10001\nWORKDIR /workspace\nENTRYPOINT [\"/bin/sh\"]\n",
    );
    let mut tar = tar::Builder::new(Cursor::new(Vec::new()));
    let mut header = tar::Header::new_gnu();
    header.set_size(dockerfile.len() as u64);
    header.set_mode(0o444);
    header.set_cksum();
    tar.append_data(&mut header, "Dockerfile", dockerfile.as_bytes())?;
    let result = GeneratedImageContext {
        content_identity: plan.recipe_sha256.clone(),
        base_digest: plan.foundation.digest.clone(),
        package_manifest: Vec::new(),
        tar: tar.into_inner()?.into_inner(),
    };
    let selected = SelectedImage {
        profile: plan.foundation.profile,
        platform: plan.platform.clone(),
        reference: String::new(), // replaced by the verified Docker image ID
        digest: String::new(),
        runtime_user: "10001:10001".into(),
        tool_versions: versions,
    };
    Ok((result, selected))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn generated(tools: &[&str]) -> (GeneratedImageContext, SelectedImage) {
        let lock = ImageLock::bundled().unwrap();
        let tools = tools.iter().map(|v| (*v).to_owned()).collect::<Vec<_>>();
        let super::super::ProjectRuntimeImagePlan::Composition(plan) =
            lock.plan_project_runtime("amd64", &tools).unwrap()
        else {
            panic!("expected a composite");
        };
        verified_context(&plan).unwrap()
    }

    #[test]
    fn dockerfile_is_generated_only_from_approved_digest_references() {
        let (context, selected) = generated(&["cargo", "node"]);
        let mut archive = tar::Archive::new(context.tar.as_slice());
        let mut dockerfile = String::new();
        archive
            .entries()
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .read_to_string(&mut dockerfile)
            .unwrap();
        assert!(dockerfile.starts_with("FROM docker.io/library/rust@sha256:"));
        assert!(dockerfile.contains("COPY --from=docker.io/library/node@sha256:"));
        assert!(!dockerfile.contains("RUN "));
        assert!(!dockerfile.contains("ADD "));
        assert!(dockerfile.contains("USER 10001:10001"));
        assert!(selected.tool_versions.contains_key("node"));
        assert!(selected.tool_versions.contains_key("cargo"));
    }

    #[test]
    fn rejects_tampered_identity_and_unapproved_image_inputs() {
        let lock = ImageLock::bundled().unwrap();
        let tools = vec!["cargo".to_owned(), "node".to_owned()];
        let super::super::ProjectRuntimeImagePlan::Composition(mut plan) =
            lock.plan_project_runtime("amd64", &tools).unwrap()
        else {
            panic!("expected a composite");
        };
        plan.components[0].reference = "docker.io/library/node:latest".into();
        assert!(verified_context(&plan).is_err());
        plan = match lock.plan_project_runtime("amd64", &tools).unwrap() {
            super::super::ProjectRuntimeImagePlan::Composition(plan) => plan,
            _ => unreachable!(),
        };
        plan.recipe_sha256 = format!("sha256:{}", "a".repeat(64));
        assert!(verified_context(&plan).is_err());
    }
}
