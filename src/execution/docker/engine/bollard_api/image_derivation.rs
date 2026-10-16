use super::super::{EngineFuture, failure};
use super::BollardApi;
use crate::execution::docker::discovery::{PrerequisiteCode, PrerequisiteFailure};
use bollard::{
    body_full,
    query_parameters::{BuildImageOptionsBuilder, BuilderVersion},
};
use futures_util::StreamExt;

mod build_diagnostics;
mod verify;

use build_diagnostics::sanitize_build_diagnostic;
pub(super) use verify::verify;

type PackageIdentity = crate::execution::docker::image_extension::builder::PackageArtifactIdentity;
type GeneratedContext = crate::execution::docker::image_extension::builder::GeneratedImageContext;

pub(super) fn image_package_state<'a>(
    api: &'a BollardApi,
    image_id: &'a str,
) -> EngineFuture<
    'a,
    Result<
        std::collections::BTreeMap<String, crate::harness::resource_bridge::InstalledPackage>,
        PrerequisiteFailure,
    >,
> {
    super::super::image_derivation::image_package_state(api, image_id)
}

pub(super) fn build<'a>(
    api: &'a BollardApi,
    context: &'a GeneratedContext,
    platform: &'a str,
) -> EngineFuture<'a, Result<String, PrerequisiteFailure>> {
    Box::pin(async move {
        if !context.content_identity.starts_with("sha256:") || !platform.starts_with("linux/") {
            return Err(failure(
                PrerequisiteCode::CapabilitiesIncompatible,
                "Authorized image build context is invalid",
                false,
            ));
        }
        let tag = format!("koolade-derived:{}", &context.content_identity[7..]);
        match api.docker.inspect_image(&tag).await {
            Ok(inspect) => {
                let id = inspect.id.unwrap_or_default();
                if valid_image_id(&id)
                    && has_identity_labels(
                        inspect.config.and_then(|config| config.labels),
                        &context.content_identity,
                        &context.base_digest,
                    )
                {
                    return Ok(id);
                }
                return Err(failure(
                    PrerequisiteCode::ImageUnavailable,
                    "Content-addressed derived image cache failed identity verification",
                    false,
                ));
            }
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => {}
            Err(_) => {
                return Err(failure(
                    PrerequisiteCode::DaemonStopped,
                    "Docker could not inspect the derived image cache",
                    false,
                ));
            }
        }
        let options = BuildImageOptionsBuilder::default()
            .dockerfile("Dockerfile")
            .t(&tag)
            .rm(true)
            // Docker's classic builder rejects networkmode. Explicitly choose
            // BuildKit so both this API option and RUN --network=none are
            // enforced instead of silently falling back to a networked build.
            .version(BuilderVersion::BuilderBuildKit)
            // The Docker Engine BuildKit API requires a session identifier;
            // Bollard uses it to establish the protocol stream, without any
            // auth or secret providers for this generated-context build.
            .session(&uuid::Uuid::new_v4().to_string())
            .networkmode("none")
            .platform(platform)
            .build();
        let mut stream =
            api.docker
                .build_image(options, None, Some(body_full(context.tar.clone().into())));
        while let Some(result) = stream.next().await {
            let result = result.map_err(|error| {
                build_failure(
                    "Docker failed to build the authorized offline system image",
                    &error.to_string(),
                )
            })?;
            if let Some(detail) = result.error_detail {
                let diagnostic = detail
                    .message
                    .as_deref()
                    .or(result.stream.as_deref())
                    .unwrap_or("Docker returned an unspecified build error");
                return Err(failure(
                    PrerequisiteCode::ImageUnavailable,
                    &format!(
                        "Docker rejected the authorized offline system image build: {}",
                        sanitize_build_diagnostic(diagnostic)
                    ),
                    false,
                ));
            }
        }
        let inspect = api.docker.inspect_image(&tag).await.map_err(|_| {
            failure(
                PrerequisiteCode::ImageUnavailable,
                "Docker built image could not be inspected",
                false,
            )
        })?;
        let id = inspect.id.unwrap_or_default();
        if !valid_image_id(&id)
            || !has_identity_labels(
                inspect.config.and_then(|config| config.labels),
                &context.content_identity,
                &context.base_digest,
            )
        {
            return Err(failure(
                PrerequisiteCode::ImageUnavailable,
                "Docker returned an invalid derived image identifier",
                false,
            ));
        }
        Ok(id)
    })
}

fn build_failure(message: &str, diagnostic: &str) -> PrerequisiteFailure {
    failure(
        PrerequisiteCode::ImageUnavailable,
        &format!("{message}: {}", sanitize_build_diagnostic(diagnostic)),
        false,
    )
}

fn has_identity_labels(
    labels: Option<std::collections::HashMap<String, String>>,
    content_identity: &str,
    base_digest: &str,
) -> bool {
    labels.is_some_and(|labels| {
        labels
            .get("io.koolade.content")
            .is_some_and(|value| value == content_identity)
            && labels
                .get("io.koolade.base-digest")
                .is_some_and(|value| value == base_digest)
    })
}

fn valid_image_id(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}
