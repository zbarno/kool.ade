# Computer Use guest application catalog and provisioning

The app catalog is a controller-owned, signed record for one immutable guest
image digest and architecture. Each entry binds an app ID and display name to
an exact package/source identity, version, package digest, publisher,
provenance, guest executable ID, capability requirements, install recipe,
removal and rollback policy, and integrity evidence. Catalog keys must come
from the application-pinned trust store. The guest image catalog and app
catalog must identify the same image digest before a session can use the app.

Providers and guests use app IDs only. They cannot choose package-manager
arguments, executable paths, installer scripts, package sources, or QEMU
settings. App operations use a controller-minted, in-memory approval token
bound to one user, task, session, app ID, operation, catalog and image digest,
architecture, lease generation, approval generation, and expiry. The signed
manifest's required capabilities must fit within the active session grant;
installing an app cannot grant network, filesystem, device, or host access.

The controller distinguishes preinstalled apps, trusted catalog entries,
operator-approved additional apps, and unsupported apps. Install, update,
remove, and launch are separate explicit operations. Updates never run
automatically. The app policy builder emits a fenced `ComputerActionV1` and
uses the app operation ID as its idempotency key, leaving replay and uncertain
outcome ownership to the existing session action journal. This contract does
not create a second app-operation store or replay mechanism.

The wire operation is `ComputerActionPayloadV1::ManageApp`, designed for the
authenticated guest action channel and its action sequence, lease,
approval-generation, cancellation, and replay fences. The action is restricted
to human control, an app in the controller's session allowlist, and signed
catalog/package digests. The controller loads an optional bounded catalog file
from app-owned storage and verifies its application-pinned signature and image
binding. The Task Details surface displays the exact package identity,
publisher, provenance, digests, and requested capabilities before exposing
operation controls. These controls remain disabled unless the guest advertises
app-management capability. The current companion advertises it as false and
returns an explicit unsupported result because the signed app package cache
and bounded privileged guest helper are not yet in the promoted image.
Provider input is never passed to a shell or package manager.

Task-built GUI applications have a separate `StagedTaskApplicationV1` contract;
they are not release-catalog packages. It binds the task revision, app target,
execution recipe, approved build-receipt hash, image digest and architecture,
executable ID and relative path, and a sorted set of P15 artifact receipts and
their transfer-grant IDs. Validation recomputes the Runtime QA build hash from
the sorted `(relative_path, bare_file_sha256)` JSON tuples, caps the set at 64
files and 64 MiB total, and requires the executable to be one of those files.
The distinct `LaunchApprovedStagedTaskApp` action requires human control and an
app allowed by the session. The authenticated virtio channel transfers the
receipt-bound bytes in ordered, bounded chunks after the action journal records
the pending operation. The separate root helper validates the same session,
lease, approval and cancellation generations, verifies every digest and the
canonical build hash, then writes a read-only per-session file tree. It accepts
only a native ELF executable matching guest architecture and launches the
relative path with empty arguments as the unprivileged desktop user. Transfer
or launch replay is denied. The controller signs the exact staged action and a
random one-use nonce with its host key; the helper verifies that signature
against the public key provisioned through read-only QEMU fw_cfg, persists the
signed action with the staged files, and rejects forged local agent IPC or
altered manifests. The same proof boundary applies to privileged app-management
operations. Task app files do not enter the signed release catalog or change
network permissions. Each P15 task-to-guest grant is minted
through a controller method that reloads the workspace-scoped
`QaComputerBuildApprovalV1`, validates it against the current request,
lifecycle revision, and Computer Use target, then binds the canonical approval
digest and active session lease/approval generations. The generic grant API
cannot claim this approval. The controller accepts only a bounded in-memory
file set whose sorted path/digest manifest matches the approved build hash,
stages each file through the shared P15 bridge, and returns typed receipts.
The Runtime QA target binds the current controller session attestation; launch
verifies the target, approval, live Human-owned session, and every artifact
grant before dispatching the typed guest action.
`ComputerSessionV1::attestation_sha256()` provides a versioned digest over the
domain-separated signed authority payload, signer key ID, and signature. #389
owns only the QA adapter that supplies the typed request/build and calls these
controller-owned methods.

The `runtime_qa/v3` Computer Use role is session-consent scoped to the fixed
`qa-task-app` ID, disabled networking, screen/window observation, pointer and
keyboard input, waits, state queries, and `LaunchApprovedStagedTaskApp`. It
cannot request catalog app launches, app install/update/remove, accessibility,
or other app IDs. The staged launch method requires a registered Human
authority after explicit control handoff; it receives the controller's existing
`ComputerArtifactBridgeV1` and rechecks all P15 grants atomically for the task
revision, session, lease, and approval generation before transferring bytes.

This version supports disposable sessions only. Personal persistent session
operations remain denied until the encrypted personal-session contract is
implemented. Unsupported examples include non-Linux programs, architectures
absent from the signed catalog, GPU-only software, and apps that depend on
DRM, anti-cheat, or unavailable hardware. Unsupported apps are reported as
such; no arbitrary Linux application compatibility is implied.

The catalog verifier, approval checks, discovery checks, and action builder
are component contracts. Runtime provisioning still requires a reviewed
catalog asset bound to the promoted image and the authenticated guest-side
package helper. The task-build launch helper is advertised only by images that
contain the isolated agent identity, helper service, private staging root, and
typed transfer protocol; the currently promoted guest image does not advertise
it. Real guest provisioning and GUI interaction are covered by the integrated
release validation in #392.
