# Machine bootstrap readiness

Tracking: [#337](https://github.com/zbarno/kool.ade/issues/337).

Kool.ad/e runs a bounded machine readiness scan from first launch, including
before a workspace or Docker daemon exists. It probes the six supported coding
agent CLIs and Git independently from Docker. Provider readiness requires a
current executable/version, authenticated account, fresh model evidence, and
safe context-only capability. A missing optional provider does not make a
selected provider unusable, and Kool.ad/e never silently selects a different
provider.

## Readiness data

`MachineSnapshotV1` is a versioned, typed snapshot. It records per-platform
capability facts, the state and source generation of each probe, and a
dependency graph that distinguishes ready, missing, unknown, stale, failed,
unsupported, blocked, and not-applicable results. Unknown or stale evidence is
never promoted to ready. Docker images are reported missing only after a
trusted, queryable engine reports its image inventory; a stopped, rootful,
inaccessible, or untrusted engine blocks image inspection instead.

The scan keeps provider output, credentials, model names, executable paths,
Docker endpoint paths, and daemon identifiers out of the persisted snapshot.
Endpoint and daemon identity fingerprints are held only in memory to reject a
result when the selected runtime changes. The worker performs a final bounded
identity check immediately before saving the result; refresh requests are
serialized with that local commit. Cancelled or stale scan generations are
discarded.

Machine setup observations live in the application state directory, not in a
project repository or shared planning artifacts. Reconciliation uses a
process mutex and OS file lock; older scans cannot replace newer observations.
An absent task is marked verified only when that exact prerequisite was
positively rechecked. A saved observation never authorizes a host mutation or
replaces an execution boundary.

## Platform and operation limits

The capability matrix marks Ubuntu 24.04 x86_64 as the current experimental
Linux controller combination. It records the Linux family and numeric major
and minor version using a fixed enum; unrecognized distribution IDs collapse
to `other`. The probe also checks subordinate UID/GID mapping files, the
current systemd user socket, and known package-manager executable locations
without storing account names or raw distro identifiers. Other Linux releases,
distributions, and architectures remain unverified. Rootless readiness also
requires a verified rootless Engine, supported cgroups, capacity, and security
evidence; systemd-user availability alone does not establish execution support.

Windows WSL2, virtualization, and Docker Desktop trust facts that have not been
directly probed remain unknown. macOS and Windows native execution is planned
and remains blocked pending native Docker Desktop staging, security, and runtime
validation. Synthetic platform fixtures validate classification; they do not
establish support on those hosts.

This readiness layer does not install packages, configure services, pull images
automatically, or execute model-provided shell commands. A supported
remediation operation must use a typed action, obtain the required operator
approval and native privilege confirmation, and be followed by an independent
readiness probe. If a coding CLI is absent or not usable, deterministic setup
instructions remain available without agent assistance. Runtime QA stays
disabled until its separate acceptance criteria pass.

## Acceptance evidence

The automated suite covers typed evidence and dependency consistency,
redaction, stale generations, exact image prerequisites, setup-task locking,
and the UI result-acceptance boundary. Release acceptance still requires
disposable native Linux checks for rootless Docker and Git/provider readiness,
plus native macOS and Windows Docker Desktop transport and security checks.
Those host checks must be recorded separately from synthetic fixture results.
