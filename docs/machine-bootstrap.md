# Machine bootstrap readiness and agent-guided setup

Tracking: [#337](https://github.com/zbarno/kool.ade/issues/337).

## Implementation status: slices A-C (incremental foundation)

Kool.ad/e starts bounded discovery of the six supported coding-agent CLIs and
Git from first launch, before a workspace or Docker daemon exists. Provider
readiness requires a current executable and version, authenticated account,
fresh model evidence, and safe context-only capability. Unknown authentication
or model access fails closed. An unavailable optional provider does not make a
selected provider unusable, and Kool.ad/e never silently selects another
provider.

## Readiness data and task lifecycle

`MachineSnapshotV1` is a versioned, typed snapshot. It records per-platform
capability facts, the state and source generation of each probe, and a
dependency graph that distinguishes ready, missing, unknown, stale, failed,
unsupported, blocked, and not-applicable results. Unknown or stale evidence is
never promoted to ready. Docker images are reported missing only after a
trusted, queryable engine reports its image inventory; a stopped, rootful,
inaccessible, or untrusted engine blocks image inspection instead.

Findings reconcile into a machine-local task collection under operator state,
outside project repositories. The versioned store migrates prior schemas,
including generic Docker and provider findings that must be rechecked before
replacement. It reserves a durable scan generation, uses atomic writes and an
inter-process lock, and bounds task, attempt, and event history. A validated
dependency graph gives stable task order, detects cycles, and projects
unresolved prerequisites as blockers. Fresh, domain-matched probe evidence is
required to verify a task; a regression reopens it. Restart recovery interrupts
in-flight work and clears active authorization bindings, so operations and
approvals are never replayed automatically.

The scan keeps provider output, credentials, model names, executable paths,
Docker endpoint paths, and daemon identifiers out of the persisted snapshot.
Endpoint and daemon identity fingerprints stay in memory to reject a result
when the selected runtime changes. A final bounded identity check and local
commit are serialized with refresh requests. Canceled or stale scan
generations are discarded. A saved observation or authorization record never
authorizes a host mutation or replaces an execution boundary.

## Welcome and task UI

The Welcome screen shows a machine-setup board before a workspace is connected.
Each active finding has its own card with its current state, prerequisites,
guidance, a rescan control, and a repair action only where a typed repair is
supported. Kanban uses the same task cards and opens the shared Task Details
shell. Details provide plain-language guidance beside structured status,
blockers, progress history, and verification evidence; narrow windows stack the
panes. Platform-specific vendor guides are linked from each task. Unsupported
platforms remain explicitly unavailable and never appear ready.

Image preparation opens a review first. Approval sends a typed application
command through the consent gate; it does not give the setup assistant
host-shell access. The setup view shows only bounded status and redacted audit
metadata. Raw host command output and credentials are not retained. Attempt and
authorization records are typed audit metadata only; they do not execute repairs,
grant host access, or authorize machine mutation.

## Safety contract

- A Docker or image finding comes only from its own typed probe evidence. An
  unavailable, stopped, rootful, or untrusted daemon does not prove an image is
  missing.
- Machine setup records live under operator-local app state. They are not added
  to `.koolade-packet`, project planning Git, task clones, or shared team state.
- Task identity is stable across workspaces on one machine. A new scan cannot
  duplicate an existing prerequisite or overwrite a newer observation.
- A verified state requires fresh authoritative evidence for that task's
  readiness domain. A model response, unrelated failure, or stale scan is not
  evidence of success.
- Persisted approval metadata does not execute an operation. Remediation must
  use a typed action, genuine user consent, any required native privilege
  prompt, and an independent readiness probe.
- The bootstrap assistant can explain choices and request typed actions. It
  cannot invent or execute host shell commands, elevate itself, or override
  Docker rootless policy. Deterministic setup guidance remains available when
  no coding CLI is installed or authenticated.
- Runtime QA remains disabled until #171 passes.

The #342 remediation broker now defines strict typed requests and exact plans,
derives the current host identity and OS version at consent and launch, and
persists authorization, bounded redacted outcomes, and post-operation probe
receipts in the setup journal. A launched operation is probed after success,
failure, timeout, or cancellation. Restart recovery marks an in-flight repair
interrupted and requires fresh setup evidence before any retry.

This broker slice registers no production host installer or service operation
and has no controller call site. Its native elevation executor remains
fail-closed, and the supervised native command runner is disabled on macOS
until its process-containment adapter is implemented. Existing consented Linux
rootless image preparation remains the #362 path. The distro-specific Linux,
macOS, and Windows host operations belong to #346, #348, and #349; they must add
exact file/service targets and their own supported-host evidence before they
can run. This PR makes no new native platform support claim.

## Platform and operation limits

The capability matrix marks Ubuntu 24.04 x86_64 as the current experimental
Linux controller combination. It records Linux family and numeric major and
minor version using a fixed enum; unrecognized distribution IDs collapse to
`other`. The probe checks subordinate UID/GID mappings, the current systemd
user socket, and known package-manager executable locations without storing
account names or raw distribution identifiers. Other Linux releases,
distributions, and architectures remain unverified. Rootless readiness also
requires a verified rootless Engine, supported cgroups, capacity, and security
evidence; systemd-user availability alone does not establish execution
support.

Windows WSL2, virtualization, and Docker Desktop trust facts that have not been
directly probed remain unknown. macOS and Windows native execution remains
blocked pending native Docker Desktop staging, security, and runtime
validation. Synthetic platform fixtures validate classification; they do not
establish support on those hosts.

This readiness layer does not install packages, configure services, pull images
automatically, or execute model-provided shell commands. A supported
remediation operation uses a typed action, required operator approval and
native privilege confirmation, and an independent readiness probe. If a coding
CLI is absent or unusable, deterministic setup instructions remain available
without agent assistance.

## Acceptance evidence and remaining work

The automated suite covers typed evidence and dependency consistency,
redaction, stale generations, exact image prerequisites, task-store lifecycle,
and the UI result-acceptance boundary. Human validation is still required for
novice first-run usability, visual review at 360/900/1280/1600 px, keyboard and
screen-reader use, reduced motion, and real-host setup behavior. Release
acceptance also requires disposable native Linux checks for rootless Docker and
Git/provider readiness; macOS Docker Desktop on Intel and Apple Silicon;
Windows WSL2 x64; signed native packaging; and the full security/usability gate
tracked by #342–#351. Mocks or Linux CI do not certify those platforms.
