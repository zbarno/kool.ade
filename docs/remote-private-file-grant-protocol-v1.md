# Remote private-file grant protocol v1

Private project files remain excluded from `WorkspaceManifestV1`. A grant is
separate user-authorized capability; it does not broaden ordinary workspace
transfer.

## Grant identity and authorization

The controller persists metadata-only receipts and revocations under the
user-local state root. A receipt binds a canonical grant ID, project/task IDs,
registered worker and host-key fingerprint, exact canonical relative paths and
SHA-256 digests, data-class labels, operator identity, confirmation time, and
expiry. File bytes never enter the receipt store. Authorization is scoped to a
project, task, worker, exact file set, and bounded validity period, and requires
explicit confirmation that names the destination, files, and data classes.

Task Details shows the destination and exact selected files before confirmation.
The controller rechecks the active receipt, source revision, excluded paths,
and source-file hashes before transfer and during execution polling. Missing,
expired, revoked, mismatched, or stale-content receipts deny transfer or result
application.

## Transfer and worker lifecycle

The controller sends typed grant-authorization metadata over the authenticated
SSH session. Private-file chunks use bounded JSON metadata followed by a
separate binary tail. They bypass the durable `remote_requests` envelope store
and are not replayed after an ambiguous send. Secret tails are read into
zeroizing buffers. Worker staging enforces worker/workspace/execution binding,
ordered offsets, exact lengths, expiry, and final digests.

The worker exposes the verified stage only to the bound Docker execution. Stage
directories are sealed to `0555` and files to `0444`; the outer worker staging
directory remains private. Cleanup restores owner write access only after the
container stops. The Start marker and parent directory are synced before Docker
start. Each execution holds an OS owner lock outside the mount. Recovery keeps
an orphaned execution's lock through Docker reconciliation, skips stages still
owned by a live worker, and only removes an orphan after its container stop is
confirmed. A two-session regression test covers the pre-Start ownership race.

During active execution, the worker stops the bound container before
acknowledging cancellation, revocation, or expiry. The worker enforces expiry
locally, including for unstarted stages. The controller checks revocation
before each chunk and holds the grant-store lock through each transmission and
the Start request. It also holds that lock through final result synchronization,
so a revocation cannot commit while authorized changes are being applied. A
revoked run is cancelled and its stale result is discarded.

## Validation and remaining proof

The gated authenticated SSH/rootless-Docker integration test covers executor
UID read-only access, active revocation, and stale-result rejection. It requires
rootless Docker, `sshd`, and the remote worker binary, and has moderate setup
cost (one worker and one task clone). Local execution was not verified on the
rootful Docker host without `sshd`; rootless SSH/Docker expiry and restart proof
remains outstanding.

Worker capability observations include the Kool.ad/e app version, Docker
versions/API, policy hash, image digests, and resource facts, but not a
toolchain version. Toolchain attribution remains `Not recorded` until a trusted
worker capability source is available.
