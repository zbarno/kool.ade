# Docker release evidence

This is a dated evidence snapshot for the Docker release gate in issue #144.
It records what the repository and hosted checks prove, and keeps unsupported
or unverified claims visible. It does not approve a release.

Reviewed 2026-10-23 at 14:36 UTC against `main` at
`e0b586c24c5e65e7b0691713f01dbdef65b18685`.
Refresh this file against the exact release candidate before making a release
decision. A pull request check is evidence for that pull request's head only;
it is not evidence that the same change is on `main`.

Post-merge [quality run #39961780419](https://github.com/zbarno/kool.ade/actions/runs/39961780419)
passed format, tests, documentation tests, Clippy, Docker/SSH coverage, and
Docker-only source checks, but the isolated queue-concurrency E2E timed out
after 181.6 seconds with tasks 001 and 002 still implementing. The required
quality aggregate failed. [Trusted image run #39961780466](https://github.com/zbarno/kool.ade/actions/runs/39961780466)
passed candidate builds and app-owned QA smoke for both profiles, but both
attestation jobs failed with `No credentials found for registry ghcr.io`.
The follow-up adds job-scoped registry authentication before attestation; its
hosted result is pending. Both QA runtime profiles remain disabled.

[PR #317 quality run #39967625398](https://github.com/zbarno/kool.ade/actions/runs/39967625398)
at head `21e32c877bd6458d246171baa269098acd420f5b` reproduced the queue E2E
timeout after 181.1 seconds. Task 001 reported an existing execution lock and
Task 002 reported an unregistered workspace after recovery attempts. This
failure is tracked by #312; a process-cleanup regression test and fake-provider
fixture fix are being prepared separately, with hosted validation pending.

Status meanings:

- **Verified**: the named behavior passed at the cited commit and environment.
- **Partial**: some named behavior passed, with a specific gap remaining.
- **Not verified**: no qualifying runtime or review evidence is recorded.
- **Pending**: the cited check is still running or review is outstanding.

## Issue status reconciliation (2026-10-23)

#143 remains closed for the implemented legacy sandbox removal/source cutover,
but the original legacy history/dirty-clone/interrupted-attempt migration
**release verification** stays in #144 and the cutover checklist. #146 has
been **reopened** because the real six-provider Docker MCP/host-code-mode
acceptance was not evidenced by the synthetic tests. #156 is separately open
for clean provider startup. Remote queue fairness and wait telemetry merged in
[#309](https://github.com/zbarno/kool.ade/pull/309); this does not establish
real two-host acceptance under #141. [#314](https://github.com/zbarno/kool.ade/pull/314)
keeps GPU/display requirements unavailable and fail-closed; it does not close
#141. [#310](https://github.com/zbarno/kool.ade/pull/310) and merged
[#311](https://github.com/zbarno/kool.ade/pull/311) split CI work. The isolated
queue-concurrency E2E passed on main at `600aa5b` but timed out at `e0b586c` in
[run #39961780419](https://github.com/zbarno/kool.ade/actions/runs/39961780419).
[Issue #312](https://github.com/zbarno/kool.ade/issues/312) remains open for the
queue regression, a green aggregate, and measured latency. The Docker-only
source assertion passed on this latest run. These updates do not retroactively
change older statuses or tested commits in the dated table.

## Evidence matrix

| Gate | Status | Evidence | Remaining proof |
| --- | --- | --- | --- |
| Pinned Rust gates and required quality aggregation | Partial | Post-merge [main run #39961780419](https://github.com/zbarno/kool.ade/actions/runs/39961780419) at `e0b586c` passed format, tests, docs, Clippy, Docker/SSH coverage, and the Docker-only assertion. The isolated queue-concurrency E2E timed out after 181.6 seconds with tasks 001 and 002 still implementing, so the required aggregate failed. It reproduced on PR #317 head `21e32c8` in [run #39967625398](https://github.com/zbarno/kool.ade/actions/runs/39967625398) after 181.1 seconds with execution-lock and clone-registration errors. The same E2E passed on main at `600aa5b`. | Resolve the queue regression, obtain a green required aggregate on the reviewed release candidate and final main, and keep #312 open until measured latency is recorded. |
| Native Linux rootless Docker integration | Partial | PR #304 and #306 hosted runs start a rootless daemon, assert the rootless container mode, and pass the Docker integration job. See [`ci.yml`](../.github/workflows/ci.yml). | The passing runs cover PR heads, not a final release candidate. Record the exact candidate run before release. |
| Authenticated SSH worker round trip | Partial | PR #304 and #306 rootless Docker jobs pass the loopback SSH integration suite, including staging and result handling. | This fixture runs on one host. It does not prove behavior across two independent Linux hosts. |
| Two controllers sharing a worker and rejecting revoked results | Partial | [`concurrent_ssh_controllers_share_worker_and_reject_revoked_result`](../src/harness/resource_bridge/tests/end_to_end/remote/two_controllers.rs) passed in PR #306's hosted run. | Full same-task/different-task, takeover, cancellation, resource contention, and independent acceptance review remain tracked by #141. |
| Two trusted workers running separate tasks concurrently | Partial | [PR #307](https://github.com/zbarno/kool.ade/pull/307) adds the rootless Docker and SSH test at head `c2763fa589176f1b511e8597b2a813ad77c71c40`. [Attempt 1](https://github.com/zbarno/kool.ade/actions/runs/39907835028/attempts/1) timed out in the existing board queue test after 181 seconds amid repeated harness-recovery and provider-rejection errors. [Attempt 2](https://github.com/zbarno/kool.ade/actions/runs/39907835028/attempts/2) passed the two-worker test and Docker daemon restart recovery E2E. | This verifies the controlled same-host fixture on the PR head. Add the final-head and release-candidate run, plus the required real two-Linux-host evidence. |
| Confirmed worker-loss retry and ownership fencing | Partial | [PR #278](https://github.com/zbarno/kool.ade/pull/278) is merged. | The broader multi-controller recovery and authorization acceptance in #141 is still open. |
| Worker-private file approval and transfer | Partial | [PR #269](https://github.com/zbarno/kool.ade/pull/269) is merged with scoped approvals and transfer coverage. | Reconcile with the full broker, cancellation, expiry, and multi-controller matrix in #141. |
| Six-provider fake launch and role/mode authority matrix | Partial | [PR #192](https://github.com/zbarno/kool.ade/pull/192) and [`harness-clean-launch-matrix.md`](harness-clean-launch-matrix.md) cover fake CLIs, every execution mode, and role authority through the application-owned bridge. | This proves application behavior against fakes only; it does not prove vendor runtime semantics. |
| Docker-backed provider role parity and host code-mode behavior ([#146](https://github.com/zbarno/kool.ade/issues/146)) | Not verified | [PR #302](https://github.com/zbarno/kool.ade/pull/302) verifies mock registrations and explicitly says it does not satisfy #146. #146 was reopened on 2026-10-23 because its original real-provider Docker MCP matrix remains unverified. | Verify eligible roles across all six actual provider CLIs, including host code-mode denial and app-owned Docker MCP. Keep this release prerequisite visible until the matrix has passing evidence. |
| Real clean-launch semantics ([#156](https://github.com/zbarno/kool.ade/issues/156)) | Not verified | [`harness-clean-launch-matrix.md`](harness-clean-launch-matrix.md) documents the fake-provider gate and provider-specific unknowns. No live provider prompt was run for this snapshot. | Verify supported real CLI authentication, ambient instruction/plugin suppression, model selection, session lifecycle, and provider-side bridge behavior before closing #156. |
| Legacy runtime source assertion | Partial | [PR #297](https://github.com/zbarno/kool.ade/pull/297) is merged; #143 is closed. The Docker-only source assertion passed on latest main at `e0b586c` in [run #39961780419](https://github.com/zbarno/kool.ade/actions/runs/39961780419). | Preserve the passing assertion on the reviewed final main commit. This does not by itself satisfy the wider release matrix in #144. |
| Filesystem and staging traversal negatives | Partial | The repository has focused rejection tests for symlinked `.git`, escaping source links, and unsafe returned archive entries, for example [`staging tests`](../src/execution/docker/sandbox/staging/tests.rs) and [`remote workspace scan tests`](../src/execution/remote/workspace/scan/tests.rs). | Map each required attack from #144 to an actual Docker-backed denial test and cite its passing hosted result. |
| Docker image build, candidate digest, and attestation | Partial | Main [image run #39961780466](https://github.com/zbarno/kool.ade/actions/runs/39961780466) built and validated both QA candidates, published the reviewed digests, and passed app-owned smoke for both profiles. Both attestation jobs then failed with `No credentials found for registry ghcr.io`, so attested evidence was not retained. Both `runtime_enabled` values remain `false`. | Provide registry credentials in the attestation job, pass a fresh trusted main run, retain the provenance/SBOM evidence, and keep runtime disabled until the wider release gate passes. |
| macOS and Windows Docker Desktop support | Not verified | [`docker-security-matrix.md`](docker-security-matrix.md) labels both platforms not production-verified. | Keep them marked experimental or unsupported until their native app, path/staging, SSH, and isolation smoke steps have sign-off. |
| Legacy state migration and resume preservation | Not verified | The cutover items remain unchecked in [`docker-cutover-checklist.md`](docker-cutover-checklist.md). | Run migration and recovery against representative legacy histories, dirty clones, interrupted executions, and retained snapshots. |
| Independent security and release review | Not verified | No release sign-off is recorded. | Complete independent review and resolve blocking findings against the exact candidate. |

## Release decision

The release gate remains **blocked**. Issues #141, #144, #146, and #156 are open;
#171 remains the separate runtime QA release gate. The current CI and fake
provider results are useful evidence, but they do not establish multi-host
remote behavior, full adversarial Docker coverage, live provider semantics,
cross-platform support, or migration acceptance.

Keep the relevant checklist items unchecked until each has a passing link or a
dated manual sign-off for the exact candidate. Do not convert a `Partial`,
`Not verified`, or `Pending` row into a release claim based on code presence.
