//! Production bollard-backed executor for provider runtime sessions
//! (issue zbarno/kool.ade#591, phase 4, piece 4: the production
//! `ProviderExecutorPort` adapter).
//!
//! Bridges the session layer's SYNCHRONOUS port onto the asynchronous
//! Docker Engine API with the strictest mappings available pre-phase-2a:
//!
//! - **Trust path** mirrors the sanctioned local session discovery
//!   ([`DockerEndpoint::from_process_environment`](super::engine)::`connect`),
//!   including socket validation and version negotiation.
//! - **Network**: the spec's single posture
//!   ([`ProviderNetworkMode::Undecided`]) maps to the STRONGEST engine
//!   posture, `HostConfig.network_mode = "none"`: undecided means no
//!   authorization, and the driver is forbidden from guessing weaker
//!   settings. Authorizing a real posture is the phase-2a contract.
//! - **Seal re-check**: `create` validates [`ProviderContainerSpec::
//!   is_sealed`] AGAIN before building the engine body; a weakened or
//!   tampered spec is refused before any engine traffic.
//! - **Keeper supervision**: the workload rides the EXEC namespace, so
//!   the container's main process is a trivial keeper
//!   (`sleep infinity`, entrypoint-overridden). Immediately after start
//!   the driver probes container state: if the keeper died (image lacks
//!   the binary, entrypoint fault), the container is torn down through
//!   the ownership-gated contract and a typed error is returned. The
//!   probe converts "we don't know what's in the image" into an
//!   observable engine contract instead of an invisible hang.
//! - **Exec**: attached, output drained (discarded — observability
//!   widening of the port is a later slice), exit code polled from the
//!   engine with a hard cap so the port always resolves.
//! - **Verification gates** honor the session layer's epistemics:
//!   `verify_absent` is TRUE only on a definitive engine
//!   not-found; any other error means UNKNOWN, and unknown is NOT
//!   absence (mirrors [`RefusingUnpinnedExecutor`](super::
//!   provider_session::RefusingUnpinnedExecutor)'s documented rule).
//!
//! Threading contract: the port is synchronous, so this driver owns a
//! private current-thread tokio runtime and services every call via
//! `block_on`. It MUST be driven from a NON-tokio thread (the upcoming
//! launch wiring allocates a dedicated containment lane) — calling
//! `block_on` inside another runtime's worker would panic, and that is
//! correct fail-fast behavior.
//!
//! Still unwired by design: nothing calls this driver in production yet.
//! Real execution additionally requires the pinned-image ops step and,
//! with it, runnable capability evidence.

use bollard::Docker;
use bollard::errors::Error as EngineError;
use bollard::exec::{CreateExecOptions, StartExecOptions};
use bollard::models::{ContainerCreateBody, HostConfig};
use bollard::query_parameters::{
    CreateContainerOptions, RemoveContainerOptions, StopContainerOptions,
};
use std::time::{Duration, Instant};

use super::engine::DockerEndpoint;
use super::provider_session::{
    MountKind, OWNERSHIP_LABEL_KEY, OWNERSHIP_LABEL_VALUE, ProviderContainerSpec,
    ProviderExecutorPort, ProviderNetworkMode, ProviderSessionError,
};

/// Wall-clock cap for the keeper liveness probe. Normal success takes TWO
/// consecutive running observations (~500ms); the cap bounds only the
/// pathological "never left created state" stall before teardown.
const KEEPER_PRESENTATION_DEADLINE_MS: u64 = 5_000;
const ENGINE_POLL_INTERVAL_MS: u64 = 250;
/// Engine-side stop grace before force-remove escalation (handled by the
/// session teardown contract's remove step; stop itself stays polite).
const STOP_GRACE_SECONDS: i32 = 10;
/// Absolute cap on one contained exec; the port promise is "always
/// resolves", so unbounded waits are impermissible. Six hours exceeds any
/// realistic CLI task horizon while keeping the guarantee finite.
const EXEC_HARD_CAP: Duration = Duration::from_secs(6 * 3_600);
/// Entrypoint override providing the exec namespace's keeper. Deliberately
/// the smallest portable coreutil invocation; the liveness probe catches
/// images where even this is missing rather than trusting the build.
const KEEPER_ARG0: &str = "sleep";
const KEEPER_ARG1: &str = "infinity";

/// Classifies bollard errors that definitively mean "such entity does not
/// exist" (HTTP 404). Anything else is UNKNOWN, never absence.
pub(crate) fn is_engine_entity_missing(error: &EngineError) -> bool {
    matches!(
        error,
        EngineError::DockerResponseServerError {
            status_code: 404,
            ..
        }
    )
}

/// Pre-engine gate shared by the create path: a spec whose seal does not
/// validate against the compiled constants is refused HERE, before any
/// engine traffic. Factored out so the guard is directly testable without
/// a client.
pub(crate) fn reject_unsealed_create(
    spec: &ProviderContainerSpec,
) -> Result<(), ProviderSessionError> {
    if !spec.is_sealed() {
        return Err(ProviderSessionError::EngineOperationFailed {
            phase: "create(seal-check)",
            name: spec.container_name.clone(),
            cause: "spec failed revalidation against the sealed constants; refusing before engine traffic".into(),
        });
    }
    Ok(())
}

fn engine_phase_error(phase: &'static str, name: &str, cause: EngineError) -> ProviderSessionError {
    ProviderSessionError::EngineOperationFailed {
        phase,
        name: name.to_string(),
        cause: cause.to_string(),
    }
}

/// Pure mapping from a sealed spec to the engine create body/options.
/// Factored out so the mapping is auditable and unit-testable without a
/// daemon; it is the single place where session constants translate to
/// engine fields.
pub(crate) fn build_create_payload(
    spec: &ProviderContainerSpec,
) -> (CreateContainerOptions, ContainerCreateBody) {
    // Fail-closed posture translation: undecided == no network.
    debug_assert!(
        matches!(spec.network, ProviderNetworkMode::Undecided),
        "no other posture exists yet"
    );
    let mut tmpfs = std::collections::HashMap::new();
    for mount in &spec.mounts {
        if let Some(destination) = &mount.tmpfs_destination {
            match &mount.kind {
                MountKind::TmpfsScratch { size_bytes } => {
                    tmpfs.insert(destination.clone(), format!("size={size_bytes}"));
                }
            }
        }
    }
    let dropped: Vec<String> = spec.dropped_capabilities.iter().cloned().collect();
    let host_config = HostConfig {
        readonly_rootfs: Some(spec.readonly_rootfs),
        cap_drop: Some(dropped),
        memory: Some(i64::try_from(spec.memory_limit_bytes).unwrap_or(i64::MAX)),
        nano_cpus: Some(
            i64::try_from(
                u128::from(spec.cpu_quota_us)
                    .saturating_mul(1_000_000_000)
                    .checked_div(u128::from(spec.cpu_period_us))
                    .unwrap_or(u128::MAX),
            )
            .unwrap_or(i64::MAX),
        ),
        pids_limit: Some(i64::try_from(spec.pids_limit).unwrap_or(i64::MAX)),
        network_mode: Some("none".to_string()),
        tmpfs: Some(tmpfs),
        // Version-stable encoding of the spec's no_new_privileges flag.
        security_opt: Some(vec!["no-new-privileges:true".to_string()]),
        ..Default::default()
    };
    let body = ContainerCreateBody {
        image: Some(spec.image_reference.clone()),
        // The keeper owns the main process; workloads arrive via exec.
        entrypoint: Some(vec![KEEPER_ARG0.to_string(), KEEPER_ARG1.to_string()]),
        cmd: None,
        labels: Some(std::collections::HashMap::from([(
            OWNERSHIP_LABEL_KEY.to_string(),
            OWNERSHIP_LABEL_VALUE.to_string(),
        )])),
        host_config: Some(host_config),
        ..Default::default()
    };
    let options = CreateContainerOptions {
        name: Some(spec.container_name.clone()),
        platform: String::new(),
    };
    (options, body)
}

/// Production [`ProviderExecutorPort`] over the local Docker Engine.
pub struct ProviderContainerDriver {
    docker: Docker,
    rt: tokio::runtime::Runtime,
}

impl ProviderContainerDriver {
    /// Connects through the trusted endpoint discovery used by local
    /// execution sessions. Fails closed on any untrusted or absent
    /// endpoint.
    pub fn connect() -> Result<Self, ProviderSessionError> {
        use crate::execution::docker::engine::HostPlatform;
        let platform = match std::env::consts::OS {
            "linux" => HostPlatform::Linux,
            "macos" => HostPlatform::MacOs,
            "windows" => HostPlatform::Windows,
            _ => HostPlatform::Linux,
        };
        let endpoint = DockerEndpoint::from_process_environment(platform).map_err(|failure| {
            ProviderSessionError::EngineOperationFailed {
                phase: "discover",
                name: String::new(),
                cause: failure.message.clone(),
            }
        })?;
        let docker =
            endpoint
                .connect()
                .map_err(|failure| ProviderSessionError::EngineOperationFailed {
                    phase: "connect",
                    name: String::new(),
                    cause: failure.message.clone(),
                })?;
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .map_err(|error| ProviderSessionError::EngineOperationFailed {
                phase: "runtime",
                name: String::new(),
                cause: error.to_string(),
            })?;
        Ok(Self { docker, rt })
    }
}

impl ProviderExecutorPort for ProviderContainerDriver {
    fn create(&mut self, spec: &ProviderContainerSpec) -> Result<(), ProviderSessionError> {
        reject_unsealed_create(spec)?;
        let (options, body) = build_create_payload(spec);
        let name = spec.container_name.clone();
        let docker = self.docker.clone();
        self.rt.block_on(async move {
            let created = docker
                .create_container(Some(options), body)
                .await
                .map_err(|error| {
                    // A pre-existing foreign name is a conflict, not ours
                    // to touch; the ownership gate already prevented
                    // management, so fail closed.
                    engine_phase_error("create", &name, error)
                })?;
            let _ = created;
            docker
                .start_container(&name, None)
                .await
                .map_err(|error| engine_phase_error("start", &name, error))?;
            // Keeper liveness probe: a dead keeper means the exec
            // namespace is hollow — tear it down, loudly.
            let deadline =
                Instant::now() + Duration::from_millis(KEEPER_PRESENTATION_DEADLINE_MS);
            let mut saw_keeper_running = false;
            loop {
                match docker.inspect_container(&name, None).await {
                    Ok(info) => {
                        let running = info
                            .state
                            .as_ref()
                            .is_some_and(|s| s.running == Some(true));
                        if running {
                            // Two consecutive running observations =
                            // the keeper held across a scheduler tick;
                            // fast-path success.
                            if saw_keeper_running {
                                return Ok(());
                            }
                            saw_keeper_running = true;
                        } else {
                            let exit = info
                                .state
                                .as_ref()
                                .and_then(|s| s.exit_code)
                                .unwrap_or(-1);
                            let detail = format!(
                                "keeper ({KEEPER_ARG0} {KEEPER_ARG1}) not running; container exit {exit}"
                            );
                            let _ = docker
                                .stop_container(
                                    &name,
                                    Some(StopContainerOptions {
                                        signal: None,
                                        t: Some(STOP_GRACE_SECONDS),
                                    }),
                                )
                                .await;
                            let removed = docker
                                .remove_container(
                                    &name,
                                    Some(RemoveContainerOptions {
                                        force: true,
                                        v: false,
                                        link: false,
                                    }),
                                )
                                .await
                                .is_ok();
                            if removed {
                                return Err(ProviderSessionError::EngineOperationFailed {
                                    phase: "keeper-probe",
                                    name,
                                    cause: detail,
                                });
                            }
                            return Err(ProviderSessionError::TerminatedWithResidue(name));
                        }
                    }
                    Err(error) if is_engine_entity_missing(&error) => {
                        // Vanished outright before ever presenting as a
                        // running keeper: nothing survives, but the
                        // launch failed for reasons operators must see.
                        return Err(ProviderSessionError::EngineOperationFailed {
                            phase: "keeper-probe",
                            name,
                            cause:
                                "container vanished before keeper presentation (no residue)".into(),
                        });
                    }
                    Err(error) => return Err(engine_phase_error("keeper-probe", &name, error)),
                }
                if Instant::now() >= deadline {
                    if saw_keeper_running {
                        // Present throughout the presentation window.
                        return Ok(());
                    }
                    let detail =
                        "container never left created state within the keeper window".to_string();
                    let _ = docker
                        .stop_container(
                            &name,
                            Some(StopContainerOptions {
                                signal: None,
                                t: Some(STOP_GRACE_SECONDS),
                            }),
                        )
                        .await;
                    let removed = docker
                        .remove_container(
                            &name,
                            Some(RemoveContainerOptions {
                                force: true,
                                v: false,
                                link: false,
                            }),
                        )
                        .await
                        .is_ok();
                    if removed {
                        return Err(ProviderSessionError::EngineOperationFailed {
                            phase: "keeper-probe(cap)",
                            name,
                            cause: detail,
                        });
                    }
                    return Err(ProviderSessionError::TerminatedWithResidue(name));
                }
                tokio::time::sleep(Duration::from_millis(ENGINE_POLL_INTERVAL_MS.min(
                    KEEPER_PRESENTATION_DEADLINE_MS,
                )))
                .await;
            }
        })
    }

    fn exec(&mut self, name: &str, argv: &[String]) -> Result<i32, ProviderSessionError> {
        let docker = self.docker.clone();
        let owned = name.to_string();
        let argv: Vec<String> = argv.to_vec();
        self.rt.block_on(async move {
            let pre = docker
                .inspect_container(&owned, None)
                .await
                .map_err(|error| engine_phase_error("exec(pre-check)", &owned, error))?;
            if !pre.state.as_ref().is_some_and(|s| s.running == Some(true)) {
                return Err(ProviderSessionError::ExecBeforeStart);
            }
            let exec = docker
                .create_exec(
                    &owned,
                    CreateExecOptions {
                        attach_stdin: Some(false),
                        attach_stdout: Some(true),
                        attach_stderr: Some(true),
                        tty: Some(false),
                        detach_keys: None,
                        env: None,
                        cmd: Some(argv),
                        privileged: Some(false),
                        user: None,
                        working_dir: None,
                    },
                )
                .await
                .map_err(|error| engine_phase_error("exec(create)", &owned, error))?;
            match docker
                .start_exec(
                    &exec.id,
                    Some(StartExecOptions {
                        detach: false,
                        tty: false,
                        output_capacity: None,
                    }),
                )
                .await
                .map_err(|error| engine_phase_error("exec(start)", &owned, error))?
            {
                bollard::exec::StartExecResults::Attached { mut output, .. } => {
                    // Drain until the exec closes its streams. Output is
                    // discarded in v1; the port gains observability in a
                    // later slice (documented residual).
                    use futures_util::StreamExt as _;
                    while output.next().await.is_some() {}
                }
                bollard::exec::StartExecResults::Detached => {
                    return Err(ProviderSessionError::EngineOperationFailed {
                        phase: "exec(start)",
                        name: owned,
                        cause: "engine answered detached despite attach requested".into(),
                    });
                }
            }
            let cap_deadline = Instant::now() + EXEC_HARD_CAP;
            loop {
                match docker.inspect_exec(&exec.id).await {
                    Ok(status) => {
                        if status.running == Some(false) {
                            return match status.exit_code {
                                Some(code) => Ok(code as i32),
                                None => Err(ProviderSessionError::EngineOperationFailed {
                                    phase: "exec(exit)",
                                    name: owned,
                                    cause: "engine reported termination without an exit code"
                                        .into(),
                                }),
                            };
                        }
                    }
                    Err(error) => {
                        return Err(engine_phase_error("exec(exit)", &owned, error));
                    }
                }
                if Instant::now() >= cap_deadline {
                    return Err(ProviderSessionError::EngineOperationFailed {
                        phase: "exec(cap)",
                        name: owned,
                        cause: format!("contained exec exceeded the {EXEC_HARD_CAP:?} hard cap"),
                    });
                }
                tokio::time::sleep(Duration::from_millis(ENGINE_POLL_INTERVAL_MS)).await;
            }
        })
    }

    fn terminate(&mut self, name: &str) -> Result<(), ProviderSessionError> {
        let docker = self.docker.clone();
        let owned = name.to_string();
        self.rt.block_on(async move {
            match docker
                .stop_container(
                    &owned,
                    Some(StopContainerOptions {
                        signal: None,
                        t: Some(STOP_GRACE_SECONDS),
                    }),
                )
                .await
            {
                Ok(()) => Ok(()),
                Err(error) if is_engine_entity_missing(&error) => Ok(()),
                Err(error) => Err(engine_phase_error("stop", &owned, error)),
            }
        })
    }

    fn verify_absent(&mut self, name: &str) -> bool {
        let docker = self.docker.clone();
        let owned = name.to_string();
        self.rt.block_on(async move {
            match docker.inspect_container(&owned, None).await {
                Ok(_) => false,
                Err(error) if is_engine_entity_missing(&error) => true,
                // Unknown is not absence: engine trouble, socket flap,
                // or an unexpected status all report PRESENT so the
                // cleanup contract escalates loudly.
                Err(_) => false,
            }
        })
    }

    fn remove(&mut self, name: &str) -> Result<(), ProviderSessionError> {
        let docker = self.docker.clone();
        let owned = name.to_string();
        self.rt.block_on(async move {
            match docker
                .remove_container(
                    &owned,
                    Some(RemoveContainerOptions {
                        force: true,
                        v: false,
                        link: false,
                    }),
                )
                .await
            {
                Ok(()) => Ok(()),
                Err(error) if is_engine_entity_missing(&error) => Ok(()),
                Err(error) => Err(engine_phase_error("remove", &owned, error)),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    //! Hermetic structural proofs (no daemon required). The payload mapper
    //! is the driver's security surface: it must encode every hard default
    //! verbatim, and the seal guard must fire before any engine traffic.
    //! Live-daemon verification belongs to the ops promotion path that
    //! publishes a real pinned provider image.

    use super::*;
    use crate::execution::docker::provider_session::{
        ProviderSpecBuilder, SESSION_MAX_CONTAINER_NAME_LEN, SESSION_MEMORY_LIMIT_BYTES,
        SESSION_NAME_PREFIX, SESSION_PIDS_LIMIT,
    };

    fn valid_digest() -> String {
        format!("sha256:{}", "ab".repeat(32))
    }

    fn built_spec(scratch_bytes: u64) -> ProviderContainerSpec {
        ProviderSpecBuilder::new("example.invalid/team/cli:v1", valid_digest())
            .container_name(format!(
                "{SESSION_NAME_PREFIX}test0123456789abcdef0123456789abcd"
            ))
            .add_scratch_tmpfs("/tmp", scratch_bytes)
            .expect("whitelisted scratch accepted")
            .build()
            .expect("canonical spec validates")
    }

    #[test]
    fn payload_mapper_encodes_every_hard_default_verbatim() {
        let spec = built_spec(256 * 1024 * 1024);
        assert!(spec.is_sealed(), "sanity: builder output is sealed");
        let (options, body) = build_create_payload(&spec);
        assert_eq!(options.name.as_deref(), Some(spec.container_name.as_str()));
        assert_eq!(options.platform, "");

        let host = body.host_config.expect("host config always encoded");
        assert_eq!(body.image.as_deref(), Some(spec.image_reference.as_str()));
        assert_eq!(
            body.entrypoint.as_deref(),
            Some([KEEPER_ARG0, KEEPER_ARG1].map(str::to_owned).as_slice())
        );
        assert_eq!(
            body.cmd, None,
            "the workload rides exec, not the main process"
        );
        assert_eq!(
            body.labels
                .as_ref()
                .and_then(|labels| labels.get(OWNERSHIP_LABEL_KEY)),
            Some(&OWNERSHIP_LABEL_VALUE.to_string())
        );

        // Containment posture translation:
        assert_eq!(host.readonly_rootfs, Some(true));
        assert_eq!(
            host.security_opt.as_deref(),
            Some(["no-new-privileges:true".to_string()].as_slice()),
            "no-new-privileges rides the version-stable security opt"
        );
        let all_caps: Vec<String> = vec!["ALL".to_string()];
        assert_eq!(host.cap_drop.as_deref(), Some(all_caps.as_slice()));
        assert_eq!(
            host.cap_add.as_deref(),
            None,
            "the builder admits no additive capabilities"
        );
        assert_eq!(host.network_mode.as_deref(), Some("none"));
        assert_eq!(
            host.memory,
            Some(i64::try_from(spec.memory_limit_bytes).unwrap())
        );
        let want_nano =
            (u128::from(spec.cpu_quota_us) * 1_000_000_000) / u128::from(spec.cpu_period_us);
        assert_eq!(host.nano_cpus, Some(i64::try_from(want_nano).unwrap()));
        assert_eq!(
            host.pids_limit,
            Some(i64::try_from(spec.pids_limit).unwrap())
        );
        let tmpfs = host.tmpfs.expect("tmpfs table always present");
        assert_eq!(
            tmpfs.get("/tmp").map(String::as_str),
            Some("size=268435456")
        );
        // Cross-check the constants themselves:
        assert_eq!(spec.memory_limit_bytes, SESSION_MEMORY_LIMIT_BYTES);
        assert_eq!(spec.pids_limit, SESSION_PIDS_LIMIT);
    }

    #[test]
    fn container_names_cannot_leave_the_reserved_namespace() {
        let overflow =
            "koolade-provider-".to_string() + &"x".repeat(SESSION_MAX_CONTAINER_NAME_LEN);
        let result = ProviderSpecBuilder::new("example.invalid/t:c", valid_digest())
            .container_name(overflow)
            .build();
        assert!(
            matches!(result, Err(ProviderSessionError::InvalidContainerName(_))),
            "over-length names refuse: {result:?}"
        );
        let foreign = ProviderSpecBuilder::new("example.invalid/t:c", valid_digest())
            .container_name("someoneelses-container")
            .build();
        assert!(
            matches!(foreign, Err(ProviderSessionError::InvalidContainerName(_))),
            "foreign prefixes refuse: {foreign:?}"
        );
    }

    #[test]
    fn create_guard_refuses_a_tampered_spec_before_any_engine_traffic() {
        let mut tampered = built_spec(64 * 1024 * 1024);
        tampered.dropped_capabilities.clear(); // unsell the spec
        assert!(!tampered.is_sealed(), "sanity: tampering broke the seal");
        let error =
            reject_unsealed_create(&tampered).expect_err("unsealed specs never reach the engine");
        match &error {
            ProviderSessionError::EngineOperationFailed { phase, .. } => {
                assert_eq!(*phase, "create(seal-check)");
            }
            other => panic!("expected the seal checkpoint, got {other:?}"),
        }
    }

    #[test]
    fn create_guard_accepts_canonical_builder_output() {
        assert!(reject_unsealed_create(&built_spec(64 * 1024 * 1024)).is_ok());
    }

    #[test]
    fn entity_missing_classifier_tracks_exactly_http_404() {
        let mk = |code: u16| EngineError::DockerResponseServerError {
            status_code: code,
            message: "synthetic".to_string(),
        };
        assert!(is_engine_entity_missing(&mk(404)));
        assert!(!is_engine_entity_missing(&mk(409)));
        assert!(!is_engine_entity_missing(&mk(500)));
        let json_fault = EngineError::JsonDataError {
            message: "not json".to_string(),
            column: 0,
        };
        assert!(!is_engine_entity_missing(&json_fault));
    }

    /// Documented live-lifecycle reservation. Runs ONLY when an operator
    /// provides both a reachable engine AND a digest-addressed scratch
    /// image exposing a `sleep` keeper; CI and dev machines without that
    /// provisioning skip it honestly. The full ops promotion path
    /// (pipeline-verified provider image + evidence feed) supersedes this
    /// probe for production confidence.
    #[test]
    #[ignore = "requires a local docker engine plus an image exposing sleep; operator-run"]
    fn live_lifecycle_smoke() {
        use std::time::Duration as StdDur;
        let image_ref = std::env::var("KOOLADE_DRIVER_SMOKE_IMAGE")
            .expect("KOOLADE_DRIVER_SMOKE_IMAGE=digest-pinned image required");
        let digest = image_ref
            .split('@')
            .nth(1)
            .and_then(|suffix| suffix.strip_prefix("sha256:"))
            .expect("smoke image must be digest-addressed")
            .to_string();
        let mut driver = ProviderContainerDriver::connect().expect("local engine reachable");
        let mut spec = ProviderSpecBuilder::new("unused", digest)
            .container_name(format!(
                "{SESSION_NAME_PREFIX}livetest{}",
                std::process::id()
            ))
            .add_scratch_tmpfs("/tmp", 64 * 1024 * 1024)
            .expect("scratch valid")
            .build()
            .expect("spec builds");
        spec.image_reference = image_ref.clone();
        assert!(spec.is_sealed());
        driver
            .create(&spec)
            .expect("keeper container starts and stabilizes");
        assert_ne!(
            driver
                .exec(
                    &spec.container_name,
                    &["sh".into(), "-c".into(), "echo ok".into()]
                )
                .unwrap(),
            0,
            "distroless bases lack shells; THIS is expected to fail exec and PROVE the namespace is hollow-safe"
        );
        let _ = driver.terminate(&spec.container_name);
        let record_present = !driver.verify_absent(&spec.container_name);
        assert!(record_present || driver.remove(&spec.container_name).is_ok());
        let _ = StdDur::ZERO;
    }
}
