# Docker release evidence

This is a dated evidence snapshot for the Docker release gate in issue #144.
It records what the repository and hosted checks prove, and keeps unsupported
or unverified claims visible. It does not approve a release.

Reviewed 2026-10-23 at 06:24 UTC against `main` at
`681017a103b59f10c14894bb455e7e17565a7e99`.
Refresh this file against the exact release candidate before making a release
decision. A pull request check is evidence for that pull request's head only;
it is not evidence that the same change is on `main`.

Status meanings:

- **Verified**: the named behavior passed at the cited commit and environment.
- **Partial**: some named behavior passed, with a specific gap remaining.
- **Not verified**: no qualifying runtime or review evidence is recorded.
- **Pending**: the cited check is still running or review is outstanding.

## Evidence matrix

| Gate | Status | Evidence | Remaining proof |
| --- | --- | --- | --- |
| Pinned Rust gates and required quality aggregation | Partial | Hosted [PR #304 run](https://github.com/zbarno/kool.ade/actions/runs/39895599702) passed at `7c5679f3d89e4bccb8235288f3d4a978a3cee884`; hosted [PR #306 run](https://github.com/zbarno/kool.ade/actions/runs/39901084075) passed at `839961577e2f023a49ed699212410a0b66ceaa41`. Both run the pinned Rust gates and the required aggregate. | Rerun on the reviewed release candidate after all changes are merged. |
| Native Linux rootless Docker integration | Partial | PR #304 and #306 hosted runs start a rootless daemon, assert the rootless container mode, and pass the Docker integration job. See [`ci.yml`](../.github/workflows/ci.yml). | The passing runs cover PR heads, not a final release candidate. Record the exact candidate run before release. |
| Authenticated SSH worker round trip | Partial | PR #304 and #306 rootless Docker jobs pass the loopback SSH integration suite, including staging and result handling. | This fixture runs on one host. It does not prove behavior across two independent Linux hosts. |
| Two controllers sharing a worker and rejecting revoked results | Partial | [`concurrent_ssh_controllers_share_worker_and_reject_revoked_result`](../src/harness/resource_bridge/tests/end_to_end/remote/two_controllers.rs) passed in PR #306's hosted run. | Full same-task/different-task, takeover, cancellation, resource contention, and independent acceptance review remain tracked by #141. |
| Two trusted workers running separate tasks concurrently | Pending | [PR #307](https://github.com/zbarno/kool.ade/pull/307) adds the rootless Docker and SSH test at head `c2763fa589176f1b511e8597b2a813ad77c71c40`. Attempt 1 of hosted run [39907835028](https://github.com/zbarno/kool.ade/actions/runs/39907835028) failed in the parallel test suite after an existing board queue test timed out at 181 seconds. Attempt 2 of the same run is still in progress as of 06:24 UTC. | Require a fresh green hosted run on the final PR head, then repeat on the release candidate. |
| Confirmed worker-loss retry and ownership fencing | Partial | [PR #278](https://github.com/zbarno/kool.ade/pull/278) is merged. | The broader multi-controller recovery and authorization acceptance in #141 is still open. |
| Worker-private file approval and transfer | Partial | [PR #269](https://github.com/zbarno/kool.ade/pull/269) is merged with scoped approvals and transfer coverage. | Reconcile with the full broker, cancellation, expiry, and multi-controller matrix in #141. |
| Six-provider fake launch and role/mode authority matrix | Partial | [PR #192](https://github.com/zbarno/kool.ade/pull/192) and [`harness-clean-launch-matrix.md`](harness-clean-launch-matrix.md) cover fake CLIs, every execution mode, and role authority through the application-owned bridge. | This proves application behavior against fakes only; it does not prove vendor runtime semantics. |
| Docker-backed provider role parity and host code-mode behavior ([#146](https://github.com/zbarno/kool.ade/issues/146)) | Not verified | [PR #302](https://github.com/zbarno/kool.ade/pull/302) verifies mock registrations and explicitly says it does not satisfy #146. The #146 acceptance reconciliation records the real-provider Docker MCP matrix as outstanding, although the issue tracker is closed. | Verify eligible roles across all six actual provider CLIs, including host code-mode denial and app-owned Docker MCP. Keep this release prerequisite visible until the matrix has passing evidence. |
| Real clean-launch semantics ([#156](https://github.com/zbarno/kool.ade/issues/156)) | Not verified | [`harness-clean-launch-matrix.md`](harness-clean-launch-matrix.md) documents the fake-provider gate and provider-specific unknowns. No live provider prompt was run for this snapshot. | Verify supported real CLI authentication, ambient instruction/plugin suppression, model selection, session lifecycle, and provider-side bridge behavior before closing #156. |
| No-Bubblewrap source and CI assertion | Verified | [PR #297](https://github.com/zbarno/kool.ade/pull/297) is merged; #143 is closed. [`assert-docker-only.sh`](../scripts/assert-docker-only.sh) rejects active Bubblewrap references, and [PR #306's Docker-only job](https://github.com/zbarno/kool.ade/actions/runs/39901084075/job/120242573688) passed. | This verifies the source assertion and cutover item; it does not by itself satisfy the wider release matrix in #144. |
| Filesystem and staging traversal negatives | Partial | The repository has focused rejection tests for symlinked `.git`, escaping source links, and unsafe returned archive entries, for example [`staging tests`](../src/execution/docker/sandbox/staging/tests.rs) and [`remote workspace scan tests`](../src/execution/remote/workspace/scan/tests.rs). | Map each required attack from #144 to an actual Docker-backed denial test and cite its passing hosted result. |
| Docker image build and candidate digest verification | Partial | The passing [PR #306 executor-image run](https://github.com/zbarno/kool.ade/actions/runs/39901084134) builds and verifies candidate images; publication and attestation jobs are skipped for an untrusted pull request. See [`executor-images.yml`](../.github/workflows/executor-images.yml). | Release-candidate publication, SBOM/provenance attestations, and verification of the exact locked digests still need release evidence. |
| macOS and Windows Docker Desktop support | Not verified | [`docker-security-matrix.md`](docker-security-matrix.md) labels both platforms not production-verified. | Keep them marked experimental or unsupported until their native app, path/staging, SSH, and isolation smoke steps have sign-off. |
| Legacy state migration and resume preservation | Not verified | The cutover items remain unchecked in [`docker-cutover-checklist.md`](docker-cutover-checklist.md). | Run migration and recovery against representative legacy histories, dirty clones, interrupted executions, and retained snapshots. |
| Independent security and release review | Not verified | No release sign-off is recorded. | Complete independent review and resolve blocking findings against the exact candidate. |

## Release decision

The release gate remains **blocked**. Issues #141, #144, and #156 are open;
#171 remains the separate runtime QA release gate. The current CI and fake
provider results are useful evidence, but they do not establish multi-host
remote behavior, full adversarial Docker coverage, live provider semantics,
cross-platform support, or migration acceptance.

Keep the relevant checklist items unchecked until each has a passing link or a
dated manual sign-off for the exact candidate. Do not convert a `Partial`,
`Not verified`, or `Pending` row into a release claim based on code presence.
