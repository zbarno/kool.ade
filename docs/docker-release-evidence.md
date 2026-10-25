# Docker release evidence

This is a dated evidence snapshot for the Docker release gate in issue #144.
It records what the repository and hosted checks prove, and keeps unsupported
or unverified claims visible. It does not approve a release.

Reviewed 2026-10-25 at 03:00 UTC against main at
b1bd69b123f1d8c3beff55116d3dc56b98d2dbe0.
Refresh this file against the exact release candidate before making a release
decision. A pull request check is evidence for that pull request's head; it is
evidence for `main` only when the reviewed tree is verified to match exactly.

## Current-main snapshot (2026-10-25)

Main `b1bd69b` includes the image-lock correction from merged PR #326 and the
OpenCode task-private config boundary test from PR #364. Exact-main [quality
run #40153402238](https://github.com/zbarno/kool.ade/actions/runs/40153402238)
passed the full Rust gates, rootless Docker/SSH, daemon recovery, queue
concurrency, Windows process-tree, and Docker-only source checks. [CodeQL run
#40153402174](https://github.com/zbarno/kool.ade/actions/runs/40153402174) and
[trusted-image run #40153402207](https://github.com/zbarno/kool.ade/actions/runs/40153402207)
also passed on this commit.

The main trusted-image push built the desktop and browser QA candidates and
passed their app-owned candidate smoke checks. The push event skipped the
PR-only reviewed-digest verifier and published-image smokes. Publication and
attestations were skipped because neither image lock changed. The
`containers/images.lock.json` file is unchanged between `7b9e518` and
`b1bd69b`.
PR #364's head tree is identical to `b1bd69b` (tree
`f9a27442d688866866db262056d01887e461672b`); its [trusted-image run
#40147642533](https://github.com/zbarno/kool.ade/actions/runs/40147642533)
validated candidate evidence and passed both published-image smoke jobs
against immutable images from the default-branch lock. Since neither lock
changed, exact candidate-to-lock equality was deferred. This is evidence for
the same source tree, but it does not prove that the newly built candidates
equal the locked images.

The release gate remains blocked on independent two-host acceptance in #141,
real provider role and launch behavior in #146 and #156, legacy migration and
resume evidence, accurate platform support, and independent release sign-off.
Both QA runtime profiles remain disabled.

## Previous main snapshot (2026-10-24; `7b9e518`)

Main `7b9e518` includes the image-lock correction from merged PR #326, the
recovery fix from PR #329, and cancellation coverage from PR #360. Exact-main
[quality run #40118483189](https://github.com/zbarno/kool.ade/actions/runs/40118483189)
passed format, Nextest, documentation tests, Clippy, rootless Docker/SSH,
daemon recovery, queue concurrency, Windows process-tree, and Docker-only source
checks. [CodeQL run #40118482961](https://github.com/zbarno/kool.ade/actions/runs/40118482961)
and [trusted-image run #40118483190](https://github.com/zbarno/kool.ade/actions/runs/40118483190)
also passed on that commit.

The trusted-image run passed candidate builds and app-owned candidate smoke for
browser and desktop QA. As a push, it did not run reviewed-digest equality or
PR-only published-image smoke. Lock detection reported
`EXECUTOR_LOCK_CHANGED=false` and `QA_LOCK_CHANGED=false`, so QA publication
and attestations were skipped. It does not establish current candidate-to-lock
equality or post-merge provenance.

PR #360 merged as `7b9e518`. Its pre-merge trusted-image run
[#40115844693](https://github.com/zbarno/kool.ade/actions/runs/40115844693)
passed QA candidate builds, app-owned candidate smoke, and published-image
smoke against the default-branch lock. The two lock-change flags were false,
so exact candidate-to-lock equality was deferred. PRs #330 and #331 are also
merged. The release gate remains blocked on
separate-host acceptance in #141, real provider role parity in #146, real
provider behavior in #156 (mock-only checks are currently authorized), legacy
migration and resume evidence, accurate platform support, and independent
security sign-off.

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

At the earlier main snapshot `56b495d2`, quality run [#40079228139](https://github.com/zbarno/kool.ade/actions/runs/40079228139), [CodeQL run #40079228226](https://github.com/zbarno/kool.ade/actions/runs/40079228226), and trusted-image run [#40079228143](https://github.com/zbarno/kool.ade/actions/runs/40079228143) passed. The quality run passed format, Nextest, documentation tests, Clippy, rootless Docker/SSH, daemon recovery, queue E2E, and Docker-only source checks.

The trusted-image run built and published both QA profiles, passed app-owned browser and desktop smoke, attested provenance and SBOMs, and retained evidence. Browser digest/recipe are sha256:a247709202b9af7244392e0fa24268bfb9279afed820d69a18a7261a0ea8c46e / sha256:5aba6bef516af941bfa53366080f28e27a02b11fbeb3508b32e7935afef681d9; desktop digest/recipe are sha256:541efb7b83f0c55f81533058fe02a7b68e1136eeb2324d21e968e9e93eb30fb6 / sha256:86e4d592426dc0877a1ce850ab152c46a0a7045da3cb84653f41d4805f4a5406. Both pairs match the checked-in image lock. The retained browser SBOM is 15,451,613 bytes with SHA-256 d61061cb9d1b948330898da5ffcfb58a4d127fb2ec63ccfdc12b8a8a896ebb19; the desktop SBOM is 15,443,084 bytes with SHA-256 608d3c5f7b9bba658d8362fe733d45ea73b2d86c80b04389beaf275f3ca259c7. The image and provenance records bind those hashes to the exact digests. The push workflow skips its PR-only reviewed-digest verifier; these candidate-to-lock and SBOM comparisons were checked against the retained artifacts. Both QA runtime profiles remain disabled.
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

## Earlier issue status reconciliation (2026-10-24)

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

At the prior main snapshot `193b8cdc`, #141, #144, and #156 remained open;
#143 was closed for the implemented Docker-only cutover. PR #326's image-lock
correction and PRs #330 and #331 were merged by that snapshot. PR #360 later
merged as current main `7b9e518`; see the current-main evidence above. The
release acceptance items remain open.

## Evidence matrix

| Gate | Status | Evidence | Remaining proof |
| --- | --- | --- | --- |
| Pinned Rust gates and required quality aggregation | Partial | Current-main [quality run #40153402238](https://github.com/zbarno/kool.ade/actions/runs/40153402238) on `b1bd69b` passed format, Nextest, docs, Clippy, rootless Docker/SSH coverage, daemon recovery, queue E2E, Windows process-tree checks, and Docker-only assertions. | Keep #312 open until measured latency is recorded; retain exact-current-main evidence for the release candidate. |
| Native Linux rootless Docker integration | Partial | Current-main [quality run #40153402238](https://github.com/zbarno/kool.ade/actions/runs/40153402238) passed rootless Docker/SSH coverage and daemon recovery; PRs #304 and #306 also cover rootless container mode. See [ci.yml](../.github/workflows/ci.yml). | These fixtures run on one host. Complete the independent two-Linux-host acceptance under #141. |
| Authenticated SSH worker round trip | Partial | Current-main [quality run #40153402238](https://github.com/zbarno/kool.ade/actions/runs/40153402238) passed rootless Docker/SSH coverage, including loopback SSH staging and result exchange. | This fixture runs on one host. It does not prove behavior across two independent Linux hosts. |
| Two controllers sharing a worker and rejecting revoked results | Partial | [`concurrent_ssh_controllers_share_worker_and_reject_revoked_result`](../src/harness/resource_bridge/tests/end_to_end/remote/two_controllers.rs) passed in PR #306's hosted run. | Full same-task/different-task, takeover, cancellation, resource contention, and independent acceptance review remain tracked by #141. |
| Two trusted workers running separate tasks concurrently | Partial | [PR #307](https://github.com/zbarno/kool.ade/pull/307) adds the rootless Docker and SSH test at head `c2763fa589176f1b511e8597b2a813ad77c71c40`. [Attempt 1](https://github.com/zbarno/kool.ade/actions/runs/39907835028/attempts/1) timed out in the existing board queue test after 181 seconds amid repeated harness-recovery and provider-rejection errors. [Attempt 2](https://github.com/zbarno/kool.ade/actions/runs/39907835028/attempts/2) passed the two-worker test and Docker daemon restart recovery E2E. | This verifies the controlled same-host fixture on the PR head. Add the final-head and release-candidate run, plus the required real two-Linux-host evidence. |
| Confirmed worker-loss retry and ownership fencing | Partial | [PR #278](https://github.com/zbarno/kool.ade/pull/278) is merged. | The broader multi-controller recovery and authorization acceptance in #141 is still open. |
| Worker-private file approval and transfer | Partial | [PR #269](https://github.com/zbarno/kool.ade/pull/269) is merged with scoped approvals and transfer coverage. | Reconcile with the full broker, cancellation, expiry, and multi-controller matrix in #141. |
| Six-provider fake launch and role/mode authority matrix | Partial | [PR #192](https://github.com/zbarno/kool.ade/pull/192) and [`harness-clean-launch-matrix.md`](harness-clean-launch-matrix.md) cover fake CLIs, every execution mode, and role authority through the application-owned bridge. | This proves application behavior against fakes only; it does not prove vendor runtime semantics. |
| Docker-backed provider role parity and host code-mode behavior ([#146](https://github.com/zbarno/kool.ade/issues/146)) | Not verified | [PR #302](https://github.com/zbarno/kool.ade/pull/302) verifies mock registrations and explicitly says it does not satisfy #146. #146 was reopened on 2026-10-23 because its original real-provider Docker MCP matrix remains unverified. | Verify eligible roles across all six actual provider CLIs, including host code-mode denial and app-owned Docker MCP. Keep this release prerequisite visible until the matrix has passing evidence. |
| Real clean-launch semantics ([#156](https://github.com/zbarno/kool.ade/issues/156)) | Not verified | [`harness-clean-launch-matrix.md`](harness-clean-launch-matrix.md) documents the fake-provider gate and provider-specific unknowns. No live provider prompt was run for this snapshot. | Verify supported real CLI authentication, ambient instruction/plugin suppression, model selection, session lifecycle, and provider-side bridge behavior before closing #156. |
| Legacy runtime source assertion | Verified | PR #297 is merged; #143 is closed. The Docker-only source assertion passed on current main `b1bd69b` in [quality run #40153402238](https://github.com/zbarno/kool.ade/actions/runs/40153402238). | Preserve this check on future reviewed main commits. It does not satisfy the wider release matrix in #144. |
| Filesystem and staging traversal negatives | Partial | The repository has focused rejection tests for symlinked `.git`, escaping source links, and unsafe returned archive entries, for example [`staging tests`](../src/execution/docker/sandbox/staging/tests.rs) and [`remote workspace scan tests`](../src/execution/remote/workspace/scan/tests.rs). | Map each required attack from #144 to an actual Docker-backed denial test and cite its passing hosted result. |
| Docker image build, candidate digest, and attestation | Partial | Current-main `b1bd69b` [trusted-image run #40153402207](https://github.com/zbarno/kool.ade/actions/runs/40153402207) passed candidate builds and app-owned smoke; the push event skipped the PR-only verifier and published-image smokes, while publication and attestations were skipped because neither lock changed. The #364 PR head has the exact same Git tree as `b1bd69b`; its [trusted-image run #40147642533](https://github.com/zbarno/kool.ade/actions/runs/40147642533) validated candidate evidence and passed both published-image smoke jobs against immutable images from the default-branch lock. Exact candidate-to-lock equality was deferred because neither lock changed. | Retain the exact-tree PR evidence and run publication/attestation on a post-merge release candidate when image locks change. Keep both QA runtimes disabled until the wider release gate passes. |
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
