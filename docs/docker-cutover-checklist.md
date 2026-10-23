# Docker cutover and release checklist

**Tracker reconciliation (2026-10-23):** #143 is closed for legacy sandbox
runtime removal and Docker-only source/documentation cutover, **not** for a final
migration/release certification. Preserve the unchecked legacy history,
dirty-clone, authorization and interrupted-run migration requirement here
under #144 until real samples pass. #146 has been reopened for real
provider role/host-code-mode acceptance, while #156 owns clean startup.
PR #311 is merged. PR #317's queue-fixture and telemetry changes are merged at
`ae4db8e`; exact-main [quality run #40010752974](https://github.com/zbarno/kool.ade/actions/runs/40010752974)
passed, including rootless Docker/SSH and the queue E2E. Issue #312 stays open
until latency is measured. PR #318's registry-authentication fix and PR #323's
SBOM size fix are merged. PR #323's fresh-head checks passed, and its exact-main
[quality run #40031345431](https://github.com/zbarno/kool.ade/actions/runs/40031345431)
and [trusted-image run #40031345416](https://github.com/zbarno/kool.ade/actions/runs/40031345416)
passed on `96bbd2fc`, including both QA image builds, immutable-digest smokes,
provenance and SBOM attestations, and evidence retention. The prior post-merge
trusted run #40029167642 was cancelled when PR #324 advanced `main` before
attestation. PR #325 has since merged as `f4f3e46b`. Exact-main [quality run
#40035158318](https://github.com/zbarno/kool.ade/actions/runs/40035158318),
[CodeQL run #40035157768](https://github.com/zbarno/kool.ade/actions/runs/40035157768),
and [trusted-image run #40035158328](https://github.com/zbarno/kool.ade/actions/runs/40035158328)
passed on that commit. The trusted run rebuilt and attested both QA images;
the current digests and retained SBOM evidence are linked below. These results
do not complete the release matrix or implicitly pass its remaining checkboxes.


This checklist is the final Docker migration gate. A checked box needs a
current test result, workflow run, or manual sign-off recorded beside it.
Keep unmet items unchecked; a mock or skipped integration test is not a
runtime sign-off.

Current hosted evidence and its limits are recorded in the [Docker execution
security matrix](docker-security-matrix.md) and the [dated release evidence
matrix](docker-release-evidence.md). Neither snapshot checks off
the release gates below.

## Application and provider paths

- [ ] Application starts when Docker is missing and explains the exact blocker.
- [ ] Repository-reading planning, implementation, verification, and QA fail
  closed when no approved Docker target is ready; context-only planning remains
  separate and cannot read a repository.
- [ ] Pi, Codex, Claude Code, OpenCode, Copilot, and Antigravity use the same
  app-owned MCP tools for equivalent execution modes and roles.
- [ ] No provider can invoke host-native repository tools or acquire extra
  authority through model, role, or task selection.
- [ ] Provider discovery isolates probes, handles hangs/crashes/cancellation,
  bounds output and child cleanup, and preserves healthy results after another
  provider fails.

## Docker and image trust

- [ ] Native Linux release validation uses a real rootless Docker Engine and
  asserts daemon rootless status; rootful Linux is rejected.
- [ ] Supported Docker API/engine versions, architecture, cgroups/limits,
  available disk, image platform, and image digest are inspected before use.
- [ ] Images are built from locked sources, pinned by architecture-specific
  digest, inspected before launch, and accompanied by provenance/SBOM and
  documented update/rebuild policy.
- [ ] Runtime rejects task-provided Dockerfiles, mutable tag-only images,
  unverified registries, arbitrary Docker endpoints, privileged settings,
  host namespace/device access, and unsupported limits.
- [ ] CI provisions the approved rootless Engine and runs Docker-backed tests
  as a required quality gate without conditional skips.

## Negative security tests

- [ ] Read attempts against private home files (`.ssh`, `.aws`), sibling
  projects, other task clones, and Docker socket are denied.
- [ ] `.git` writes, symlink/parent traversal, bind-mount escape, host mount
  injection, and unsafe workspace staging are denied.
- [ ] Agent-selected image/Dockerfile, `--privileged`, capability/device
  escalation, unconfined seccomp/AppArmor, host network/PID/IPC, and unapproved
  registry access are denied.
- [ ] DNS, HTTP egress, loopback-to-host, and package install scripts that
  require unapproved network access fail.
- [ ] CPU, memory, process, temp disk, log, artifact, and wall-clock quotas are
  enforced by the Engine/runtime rather than shell `ulimit` alone.
- [ ] Cancellation cannot kill unrelated processes or containers; daemon
  restart/disconnect never becomes a successful result without an exit record.
- [ ] Forged broker capabilities, cache poisoning, stale artifact replay,
  unapproved private configuration, SSH host-key bypass, claim takeover, and
  raw remote Docker TCP are rejected.

## Positive lifecycle and integration tests

- [ ] Rust build/test and Node install/build/test complete through approved
  offline broker/cache flows.
- [ ] Planning is read-only; implementation and verification retain their
  intended task-clone writes while keeping Git metadata protected.
- [ ] Concurrent task clones remain isolated through start, cancellation,
  retry, resume, crash recovery, and cleanup.
- [ ] All six providers have fake structured-output tests across each mode and
  user role; real-provider Docker MCP checks are recorded separately where
  credentials and CI allow.
- [ ] Local browser QA can produce bounded, hash-checked screenshot evidence
  and cannot publish changes.
- [ ] A controlled SSH worker test verifies host-key trust, rootless Docker,
  stage-in/out, worker disconnect/reconnect, cancellation, stale result
  rejection, broker authorization, and task clone preservation.
- [ ] Two-controller/two-worker tests cover the same task, different tasks,
  lease takeover, worker loss, resource contention, and result-apply fencing.
- [ ] A protected or scheduled real two-Linux-host run verifies remote
  rootless Docker, concurrent execution, and disconnection recovery.

## User experience, migration, and support claims

- [ ] Settings identify local Docker readiness, daemon/image details,
  resource capacity, safe setup steps, and a cleanup-scoped smoke test.
- [ ] Remote worker registration pins SSH host identity; trust/revoke,
  enable/disable, limits, health, and active tasks are visible.
- [ ] Task Details displays selected target, transfer/run/cancel state,
  failures, and evidence without exposing private SSH configuration.
- [ ] Private files are never forwarded by default; any supported exception
  has exact worker/task/file scope, expiry, revoke, encryption, and cleanup.
- [ ] Existing task clones, dirty changes, branches, approvals, evidence,
  history, and authorization survive migration. Old runtime attempts become
  interrupted and resume only as a new Docker execution.
- [ ] Linux, macOS Docker Desktop, and Windows Docker Desktop claims match
  platform tests. Unsupported platforms are labeled experimental/unsupported.
- [ ] Threat model explains that Docker is not a perfect hostile-code boundary
  and that daemon/socket/mount authority is powerful.
- [ ] Backup, rollback, troubleshooting, worker operator, and image update
  instructions match actual behavior.

## Required final evidence

- [ ] `cargo +1.98.1 fmt --all --check`
- [ ] `cargo +1.98.1 nextest run --locked --all-targets --no-fail-fast`
- [ ] `cargo +1.98.1 test --locked --doc`
- [ ] `cargo +1.98.1 clippy --locked --all-targets -- -D warnings`
- [ ] Rootless Docker integration workflow and required quality aggregation
  passed on the exact reviewed commit.
- [ ] Independent security and code review completed; blocking findings fixed.
- [ ] Public release notes include verified platform support, migration
  behavior, and known limits.

## Previous CI evidence (2026-10-22)

At commit `08ee5bdd4cbff96b0b499e44b6237fcf93265ce1`, the hosted
[quality workflow](https://github.com/zbarno/kool.ade/actions/runs/39793292401)
passed after rerunning the initially failing test attempt. Its
[tests and Docker runtime job](https://github.com/zbarno/kool.ade/actions/runs/39793292401/job/119886403621)
provisioned Docker Engine 29.9.0 in rootless mode, asserted the Linux engine
version and rootless security option, required the SSH E2E setup, ran the full
nextest suite with Docker and remote-backend integration enabled, and passed
the daemon-restart recovery E2E. The hosted format, documentation, clippy,
Windows process-tree, Docker-only, and required quality aggregation jobs also
passed on that commit.

This records CI evidence for that revision only. It does not complete the
two-Linux-host release run, real-provider checks, platform QA, or the other
unchecked release gates above. The checklist remains blocked pending those
applicable results and a fresh passing run on the final reviewed commit.

## Latest main CI evidence (2026-10-23)

PR #317 merged at `ae4db8ea458c64392e02046be77a1f8a0bd06ab7`. Its fresh PR
quality, trusted-image, and CodeQL checks passed on the reviewed head; the
exact-main [quality run #40010752974](https://github.com/zbarno/kool.ade/actions/runs/40010752974)
also passed format, Nextest, documentation tests, Clippy, rootless Docker/SSH,
daemon recovery, the corrected queue-concurrency E2E, and Docker-only source
checks.

PR #288 merged at `652065b1`. Exact-main [quality run #40014412383](https://github.com/zbarno/kool.ade/actions/runs/40014412383)
also passed the four Rust gates, rootless Docker/SSH, daemon recovery, queue
E2E, and Docker-only source checks.

PR #323 merged as `1123eef403c96850541a048b73306d49649d1f36`. The first
post-merge trusted run [#40029167642](https://github.com/zbarno/kool.ade/actions/runs/40029167642)
was cancelled when PR #324 advanced `main` before attestation. Fresh exact-main
quality [run #40031345431](https://github.com/zbarno/kool.ade/actions/runs/40031345431),
CodeQL [run #40031345364](https://github.com/zbarno/kool.ade/actions/runs/40031345364),
and trusted-image [run #40031345416](https://github.com/zbarno/kool.ade/actions/runs/40031345416)
passed on `96bbd2fc`. The trusted run built and published both QA images,
passed app-owned smoke on each immutable digest, attested provenance and SBOMs,
and retained the attested evidence artifacts. The browser image digest is
`sha256:a247709202b9af7244392e0fa24268bfb9279afed820d69a18a7261a0ea8c46e`;
its 15,451,613-byte SBOM hash is
`sha256:9b7afa659a83775c48a53110e787a334372129ef9b7c5f2a06c64113d4831a70`.
The desktop image digest is
`sha256:d946f50f91421289799d796cb29768f0c9644f8fb05baff44048b8e7c197b0f3`;
its 15,443,084-byte SBOM hash is
`sha256:bf379772ed354a544f2266812d24d18917e369fbacfb941f73d17f26b66ab3ac`.
Both runtime profiles remain disabled.

PR #325 merged at `f4f3e46b608589882ad790b569271d46e0fa2b80`. Fresh exact-main
[quality run #40035158318](https://github.com/zbarno/kool.ade/actions/runs/40035158318),
[CodeQL run #40035157768](https://github.com/zbarno/kool.ade/actions/runs/40035157768),
and [trusted-image run #40035158328](https://github.com/zbarno/kool.ade/actions/runs/40035158328)
passed. The trusted run rebuilt and published both QA images, passed app-owned
smoke on each immutable digest, attested provenance and SBOMs, and retained
the evidence artifacts. Browser artifact
`attested-browser_qa-f4f3e46b608589882ad790b569271d46e0fa2b80` contains the
15,451,613-byte SBOM with SHA-256
`sha256:a5bd7e5d33a7c0103dc1038962f937e361f178961e006ec104379e481b256370`.
Desktop artifact
`attested-desktop_qa-f4f3e46b608589882ad790b569271d46e0fa2b80` contains the
15,443,084-byte SBOM with SHA-256
`sha256:1a24adec2d35fc352faf9da843daf42bd8a893a6829a22c8939ab64ee5ed6ad2`.
The immutable image digests are unchanged from the previous attested run, and
both runtime profiles remain disabled.

These results do not complete the real two-Linux-host run, real-provider
clean-launch and role-parity checks, migration/resume matrix, cross-platform
sign-off, or independent final release review. Keep the applicable release
boxes unchecked and the release decision blocked.

**Release decision:** blocked until all applicable items above have linked,
current evidence. Do not infer sign-off from code presence, a passing fake
engine test, or an integration job that skipped.
