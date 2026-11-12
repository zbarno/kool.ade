# Remote Computer Use workers (v1)

Remote VM workers are experimental and unavailable to product scheduling. The
`remote_computer_use_release_gate_open_v1()` gate remains closed until the
separate-host recovery, isolation, and two-operator acceptance is certified.
The local Linux R1 path remains the default. A remote target never silently
falls back to a local host.

## Trust and transport

Computer Use enrollment is separate from Docker execution-worker enrollment.
The controller pins a worker ID, SSH alias and principal, host-key fingerprint,
worker attestation key, supported protocols, image digests, resource limits,
and an explicit absolute path to the worker's private configuration file.
SSH host keys are checked against the explicitly selected `known_hosts` file.
The fixed SSH command starts only `computer-use-worker serve --stdio`; the
remote config path is validated to contain no shell metacharacters or parent
directory components.

An operator imports a separately provisioned enrollment record and chooses an
enrolled worker explicitly:

```text
koolade computer-use-target enroll --config /absolute/path/enrollment.json
koolade computer-use-target select --worker-id worker-id
koolade computer-use-target recover --worker-id worker-id --session-id <uuid> --approve
```

The enrollment store is private to the current Kool.ad/e state directory.
Existing worker IDs cannot be rebound to a different machine, host key, or
attestation key. The app's consent view carries and displays the exact typed
target; the signed session must match it. The selection command does not open
the product release gate.

Recovery is a separate explicit operator command. It signs a new lease with
advanced lease, cancellation, and approval generations, then sends one sequence
zero recovery request. It never resends a pending guest input. When the prior
action outcome is unknown, further guest actions remain blocked and the command
reports that boundary.

The worker config is a regular file owned by the worker account with mode
`0600` or stricter. It includes its worker-only attestation seed, the
controller's public verification key, allowed image digests and quotas, and
explicit catalog, cache, anti-rollback, overlay, runtime, cgroup, and
checkpoint paths. It must not contain model credentials. The worker loads the
application-pinned guest-image trust root, verifies the signed catalog and
durable generation floor, checks the session signature and target policy, then
creates a local overlay from its own verified cache. No controller overlay
path crosses the protocol.

The bounded framed stream carries lifecycle operations, display data and
guest actions. The controller keeps task approval and its action journal;
the worker keeps the VM process, authenticated guest channel, local action
journal, and durable session fence. Reconnection requires a newly signed,
generation-advancing session. Sequence zero is reserved for that recovery
request and the worker resets the new transport stream to sequence one after
acceptance. An action with an uncertain outcome is never replayed.

## Worker invocation

On a Linux host provisioned with KVM, delegated cgroup controls, the approved
guest image cache, the signed catalog, and initialized anti-rollback state,
start the worker with the private configuration explicitly named:

```text
koolade computer-use-worker serve --stdio --config /absolute/path/worker.json
```

The service fails closed when any required path, trust key, host capability,
catalog, or checkpoint is missing or unsafe. It does not enroll itself or
infer trust from Docker worker health.

## Validation boundary

Unit fixtures exercise identity, protocol, resource and fence checks. They do
not prove that independently hosted machines, real SSH hosts, separate
operators, or network interruption were tested. Product scheduling remains
disabled until those checks pass and the release gate is deliberately
certified. Remote persistent personal sessions are out of scope.

The independent-worker stdio fixture test starts two separately configured
worker servers with distinct IDs, machine identities, host fingerprints and
attestation keys. It verifies that each session stays on its enrolled worker,
that a wrong host fingerprint is rejected before an operation, and that a
session signed for one worker cannot start on the other. These are in-process
fixtures over separate streams, not physical hosts or real SSH connections.

The guest companion and QEMU backend currently have no file-transfer channel.
The controller artifact bridge enforces grant, task revision, lease,
approval-generation, digest, expiry, and store-quota checks, but remote worker
artifact bytes are not yet routed through it. Remote artifact transfer is an
open implementation gap, not a supported capability.
