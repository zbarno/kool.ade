use super::super::images::{RuntimeProfile, SelectedImage};
use crate::execution::contract::{DependencyCacheEcosystemV1, ExecutionRequestV1};

pub(super) fn variables(request: &ExecutionRequestV1, image: &SelectedImage) -> Vec<String> {
    let mut environment = vec![
        "HOME=/home/executor".into(),
        "CI=true".into(),
        "LANG=C".into(),
        "TZ=UTC".into(),
        "PIP_CONFIG_FILE=/dev/null".into(),
        "PIP_NO_INDEX=1".into(),
        "PIP_FIND_LINKS=/home/executor/.cache/koolade/pip/wheels".into(),
    ];
    if image.profile == RuntimeProfile::Rust {
        // The approved Rust image sets CARGO_HOME to /usr/local/cargo. Broker
        // snapshots are staged under the executor's private home instead.
        environment.push("CARGO_HOME=/home/executor/.cargo".into());
    }
    if image.profile == RuntimeProfile::Go {
        // /tmp intentionally stays noexec. Go creates temporary test and run
        // binaries here and removes them when the command finishes.
        environment.push("GOTMPDIR=/workspace".into());
    }
    if request
        .dependency_cache_refs
        .iter()
        .any(|cache_ref| cache_ref.ecosystem == DependencyCacheEcosystemV1::Go)
    {
        environment.extend([
            "GOPATH=/home/executor/go".into(),
            "GOMODCACHE=/home/executor/go/pkg/mod".into(),
            "GOPROXY=off".into(),
            "GOSUMDB=off".into(),
            "GOTOOLCHAIN=local".into(),
            "GOENV=off".into(),
            "GOWORK=off".into(),
            "GOPRIVATE=".into(),
            "GONOPROXY=".into(),
            "GONOSUMDB=".into(),
            "GOINSECURE=".into(),
        ]);
    }
    environment
}
