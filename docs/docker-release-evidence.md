# Docker release evidence

This is a dated evidence snapshot for the Docker release gate in issue #144.
It records what the repository and hosted checks prove, and keeps unsupported
or unverified claims visible. It does not approve a release.

Reviewed 2026-10-23 at 23:25 UTC against `main` at
`f4f3e46b608589882ad790b569271d46e0fa2b80`.
Refresh this file against the exact release candidate before making a release
decision. A pull request check is evidence for that pull request's head only;
it is not evidence that the same change is on `main`.

PR #318's isolated registry HOME fix is merged at `1b3edc5d`. PR #317's
telemetry assertions and deterministic queue fixture are merged at
`ae4db8ea`; its [fresh PR quality run #40009126794](https://github.com/zbarno/kool.ade/actions/runs/40009126794),
[trusted-image run #40009126695](https://github.com/zbarno/kool.ade/actions/runs/40009126695),
and [CodeQL run #40009122053](https://github.com/zbarno/kool.ade/actions/runs/40009122053)
passed before merge. The post-merge [quality run #40010752974](https://github.com/zbarno/kool.ade/actions/runs/40010752974)
passed format, Nextest, documentation tests, Clippy, rootless Docker/SSH,
daemon recovery, the corrected queue-concurrency E2E, and Docker-only source
checks on main commit `ae4db8ea`. PR #288 then merged at `652065b1`; exact-main
[quality run #40014412383](https://github.com/zbarno/kool.ade/actions/runs/40014412383)
passed on that current commit.

The post-merge [trusted-image run #40014412495](https://github.com/zbarno/kool.ade/actions/runs/40014412495)
on main `652065b1` passed candidate builds, QA image publication, exact-digest
app-owned smoke, and provenance attestations, but both SBOM attestations
exceeded the pinned 16 MiB limit. The retained browser and desktop SBOMs were
17,040,013 and 17,031,244 bytes. PR [#323](https://github.com/zbarno/kool.ade/pull/323)
merged as `1123eef403c96850541a048b73306d49649d1f36`; it configures Syft to emit
SHA-256-only file metadata digests, adds a fail-closed size guard, and refreshes
both reviewed QA recipe hashes from validated candidate evidence. The PR's
local four-gate run, hosted quality, trusted-image, and CodeQL checks passed.
Its first post-merge trusted-image run was cancelled when PR #324 advanced main
before attestation. Fresh exact-main [quality run #40031345431](https://github.com/zbarno/kool.ade/actions/runs/40031345431),
[CodeQL run #40031345364](https://github.com/zbarno/kool.ade/actions/runs/40031345364),
and [trusted-image run #40031345416](https://github.com/zbarno/kool.ade/actions/runs/40031345416)
then passed on `96bbd2fc`. Both immutable QA image digests passed app-owned
smoke, provenance and SBOM attestations, and evidence retention. The retained
artifacts contain a 15,451,613-byte browser SBOM (`sha256:9b7afa659a83775c48a53110e787a334372129ef9b7c5f2a06c64113d4831a70`) and a 15,443,084-byte desktop SBOM (`sha256:bf379772ed354a544f2266812d24d18917e369fbacfb941f73d17f26b66ab3ac`); their hashes match the image and provenance records. Both QA runtime profiles remain disabled.

PR #325 merged at `f4f3e46b608589882ad790b569271d46e0fa2b80`. Fresh exact-main
[quality run #40035158318](https://github.com/zbarno/kool.ade/actions/runs/40035158318),
[CodeQL run #40035157768](https://github.com/zbarno/kool.ade/actions/runs/40035157768),
and [trusted-image run #40035158328](https://github.com/zbarno/kool.ade/actions/runs/40035158328)
passed. The trusted run rebuilt and published both QA images, passed app-owned
smoke on each immutable digest, attested provenance and SBOMs, and retained
the evidence artifacts. Browser artifact
`attested-browser_qa-f4f3e46b608589882ad790b569271d46e0fa2b80` contains the
15,451,613-byte SBOM (`sha256:a5bd7e5d33a7c0103dc1038962f937e361f178961e006ec104379e481b256370`);
desktop artifact
`attested-desktop_qa-f4f3e46b608589882ad790b569271d46e0fa2b80` contains the
15,443,084-byte SBOM (`sha256:1a24adec2d35fc352faf9da843daf42bd8a893a6829a22c8939ab64ee5ed6ad2`).
Those hashes match each image and provenance record. The immutable image
digests are unchanged from the previous attested run. Both QA runtime profiles
remain disabled.

[PR #317 quality run #39998474373](https://github.com/zbarno/kool.ade/actions/runs/39998474373)
at head `7a1cacb948b39e326b16415f9e532ece87824377` passed the other required
jobs but its queue E2E timed out after 182.3 seconds. Tasks 001 and 002 were
held behind the fixture's capacity-release marker after task 004 had already
reached approval, so the expected capacity-wait state was never observed. The
deterministic fixture-ordering correction passed the isolated queue E2E
and required quality aggregate on PR #317 head
`76aec3b33b48d2d590a67acad5856e9b88bc1dff` in [run #40005864459](https://github.com/zbarno/kool.ade/actions/runs/40005864459).
The trusted-image builds and app-owned smoke checks passed on that PR head in
[run #40005864422](https://github.com/zbarno/kool.ade/actions/runs/40005864422),
and CodeQL passed in [run #40005857704](https://github.com/zbarno/kool.ade/actions/runs/40005857704).
The later reviewed PR head `4c271bd0` passed fresh quality, trusted-image, and
CodeQL checks and merged at `ae4db8ea`; exact-main evidence is linked above.
The earlier `7a1cacb9` PR head and main commit `1b3edc5d` both failed only the
queue E2E because task 004 reached approval before the test observed a capacity
waiter; their required aggregates failed as a consequence. This differs from
the earlier lock and clone-registration failure, which #319 has since fixed
and passed on main.

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
[#311](https://github.com/zbarno/kool.ade/pull/311) split CI work. PR #317
merged its deterministic queue fixture and transfer-byte assertions at
`ae4db8ea`; exact-main quality run #40010752974 passed. The earlier main run
at `1b3edc5d` failed the queue E2E before the correction merged. [Issue #312](https://github.com/zbarno/kool.ade/issues/312)
remains open for measured
latency. The Docker-only source assertion passed in the exact-main quality
run. These updates do not retroactively change older statuses or tested
commits in the dated table.

## Evidence matrix

| Gate | Status | Evidence | Remaining proof |
| --- | --- | --- | --- |
| Pinned Rust gates and required quality aggregation | Partial | Exact-main [quality run #40035158318](https://github.com/zbarno/kool.ade/actions/runs/40035158318) at `f4f3e46b` passed format, Nextest, docs, Clippy, rootless Docker/SSH coverage, daemon recovery, queue E2E, and Docker-only assertions. Earlier main run [#40009090009](https://github.com/zbarno/kool.ade/actions/runs/40009090009) at `1b3edc5d` failed only the queue E2E because the capacity waiter was not observed before task 004 reached approval; PR #317's correction is now merged and passes on main. | Keep #312 open until measured latency is recorded; retain current-main evidence for the exact release candidate. |
| Native Linux rootless Docker integration | Partial | Exact-main [quality run #40035158318](https://github.com/zbarno/kool.ade/actions/runs/40035158318) on `f4f3e46b` passed rootless Docker/SSH coverage and daemon recovery; PR #304 and #306 also cover the rootless container mode. See [`ci.yml`](../.github/workflows/ci.yml). | These fixtures run on one host. Complete the independent two-Linux-host acceptance under #141. |
| Authenticated SSH worker round trip | Partial | Exact-main [quality run #40035158318](https://github.com/zbarno/kool.ade/actions/runs/40035158318) passed rootless Docker/SSH coverage, including the loopback SSH staging and result exchange. | This fixture runs on one host. It does not prove behavior across two independent Linux hosts. |
| Two controllers sharing a worker and rejecting revoked results | Partial | [`concurrent_ssh_controllers_share_worker_and_reject_revoked_result`](../src/harness/resource_bridge/tests/end_to_end/remote/two_controllers.rs) passed in PR #306's hosted run. | Full same-task/different-task, takeover, cancellation, resource contention, and independent acceptance review remain tracked by #141. |
| Two trusted workers running separate tasks concurrently | Partial | [PR #307](https://github.com/zbarno/kool.ade/pull/307) adds the rootless Docker and SSH test at head `c2763fa589176f1b511e8597b2a813ad77c71c40`. [Attempt 1](https://github.com/zbarno/kool.ade/actions/runs/39907835028/attempts/1) timed out in the existing board queue test after 181 seconds amid repeated harness-recovery and provider-rejection errors. [Attempt 2](https://github.com/zbarno/kool.ade/actions/runs/39907835028/attempts/2) passed the two-worker test and Docker daemon restart recovery E2E. | This verifies the controlled same-host fixture on the PR head. Add the final-head and release-candidate run, plus the required real two-Linux-host evidence. |
| Confirmed worker-loss retry and ownership fencing | Partial | [PR #278](https://github.com/zbarno/kool.ade/pull/278) is merged. | The broader multi-controller recovery and authorization acceptance in #141 is still open. |
| Worker-private file approval and transfer | Partial | [PR #269](https://github.com/zbarno/kool.ade/pull/269) is merged with scoped approvals and transfer coverage. | Reconcile with the full broker, cancellation, expiry, and multi-controller matrix in #141. |
| Six-provider fake launch and role/mode authority matrix | Partial | [PR #192](https://github.com/zbarno/kool.ade/pull/192) and [`harness-clean-launch-matrix.md`](harness-clean-launch-matrix.md) cover fake CLIs, every execution mode, and role authority through the application-owned bridge. | This proves application behavior against fakes only; it does not prove vendor runtime semantics. |
| Docker-backed provider role parity and host code-mode behavior ([#146](https://github.com/zbarno/kool.ade/issues/146)) | Not verified | [PR #302](https://github.com/zbarno/kool.ade/pull/302) verifies mock registrations and explicitly says it does not satisfy #146. #146 was reopened on 2026-10-23 because its original real-provider Docker MCP matrix remains unverified. | Verify eligible roles across all six actual provider CLIs, including host code-mode denial and app-owned Docker MCP. Keep this release prerequisite visible until the matrix has passing evidence. |
| Real clean-launch semantics ([#156](https://github.com/zbarno/kool.ade/issues/156)) | Not verified | [`harness-clean-launch-matrix.md`](harness-clean-launch-matrix.md) documents the fake-provider gate and provider-specific unknowns. No live provider prompt was run for this snapshot. | Verify supported real CLI authentication, ambient instruction/plugin suppression, model selection, session lifecycle, and provider-side bridge behavior before closing #156. |
| Legacy runtime source assertion | Verified | [PR #297](https://github.com/zbarno/kool.ade/pull/297) is merged; #143 is closed. The Docker-only source assertion passed on current main `f4f3e46b` in [quality run #40035158318](https://github.com/zbarno/kool.ade/actions/runs/40035158318). | Preserve this check on future reviewed main commits. It does not by itself satisfy the wider release matrix in #144. |
| Filesystem and staging traversal negatives | Partial | The repository has focused rejection tests for symlinked `.git`, escaping source links, and unsafe returned archive entries, for example [`staging tests`](../src/execution/docker/sandbox/staging/tests.rs) and [`remote workspace scan tests`](../src/execution/remote/workspace/scan/tests.rs). | Map each required attack from #144 to an actual Docker-backed denial test and cite its passing hosted result. |
| Docker image build, candidate digest, and attestation | Verified | Trusted-main [run #40035158328](https://github.com/zbarno/kool.ade/actions/runs/40035158328) on `f4f3e46b` passed browser and desktop builds, immutable digest app-owned smoke, provenance and SBOM attestations, and evidence retention. Retained artifacts `attested-browser_qa-f4f3e46b608589882ad790b569271d46e0fa2b80` and `attested-desktop_qa-f4f3e46b608589882ad790b569271d46e0fa2b80` contain 15,451,613-byte and 15,443,084-byte SBOMs (`sha256:a5bd7e5d33a7c0103dc1038962f937e361f178961e006ec104379e481b256370` and `sha256:1a24adec2d35fc352faf9da843daf42bd8a893a6829a22c8939ab64ee5ed6ad2`) whose SHA-256 hashes match the image and provenance records. `runtime_enabled` remains `false` for both profiles. | Keep runtime disabled until the wider release gate passes. |
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
