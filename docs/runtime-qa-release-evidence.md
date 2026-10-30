# Runtime QA release evidence

## Current snapshot (2026-10-30, 21:04 UTC)

Current `main` is [`24291aa9`](https://github.com/zbarno/kool.ade/commit/24291aa9a7620a30615c2ef1c6c1c586cd354602).
Its exact-main [quality](https://github.com/zbarno/kool.ade/actions/runs/41115005725),
[CodeQL](https://github.com/zbarno/kool.ade/actions/runs/41115005003),
[native package](https://github.com/zbarno/kool.ade/actions/runs/41115005634),
and [trusted executor image](https://github.com/zbarno/kool.ade/actions/runs/41115005668)
workflows completed successfully. Runtime QA profile publication remains disabled;
passing candidate checks do not approve release or enable either profile.

The two open PRs are refreshed to this exact base:

| PR | Head | Completed checks on current head | Review state |
| --- | --- | --- | --- |
| [#435](https://github.com/zbarno/kool.ade/pull/435) | [`5f1a0aaa`](https://github.com/zbarno/kool.ade/commit/5f1a0aaa9dec52da1240ac9f215d9c76617f121f) | [Quality](https://github.com/zbarno/kool.ade/actions/runs/41120949792), [CodeQL](https://github.com/zbarno/kool.ade/actions/runs/41120940337), [five native package targets](https://github.com/zbarno/kool.ade/actions/runs/41120949925), [browser and desktop rootless candidate verification, published-image smokes, and digest verification](https://github.com/zbarno/kool.ade/actions/runs/41120950002) passed. | Maintainer review requested; auto-merge is enabled with the normal merge method. |
| [#436](https://github.com/zbarno/kool.ade/pull/436) | [`57f1d6b7`](https://github.com/zbarno/kool.ade/commit/57f1d6b77912f71512c19aa59b8ff2e9f714dde2) | [Quality](https://github.com/zbarno/kool.ade/actions/runs/41118869450), [CodeQL](https://github.com/zbarno/kool.ade/actions/runs/41118864740), [five native package targets](https://github.com/zbarno/kool.ade/actions/runs/41118869545), and [trusted-image digest verification](https://github.com/zbarno/kool.ade/actions/runs/41118869588) passed. Profile publication and published-image smokes were skipped while the profiles remain disabled. | Maintainer review requested; auto-merge is enabled with the normal merge method. |

These results establish candidate and exact-main CI status only. The remaining
technical #171 acceptance criteria are still open, and deferred human
readiness/status/screenshot review remains a prelaunch activity that does not
block issue closure after technical acceptance is complete.

## Historical exact-main snapshot (2026-10-30, 15:59 UTC)

At that timestamp, evidence was anchored to `main` commit
[`52504d90`](https://github.com/zbarno/kool.ade/commit/52504d90ea56913acbf79a5c8e307a70f1a85321),
which merged PR #433. The native package workflow
[41076371385](https://github.com/zbarno/kool.ade/actions/runs/41076371385)
passed all five targets: Linux x86_64, Linux arm64, Windows x64, Apple Silicon
macOS, and Intel macOS. The exact-main quality workflow
[41076371470](https://github.com/zbarno/kool.ade/actions/runs/41076371470)
failed only its isolated rootless-daemon recovery job on attempts 1–4 before
assertions with `Docker Engine disconnected or did not answer its health
request`; all other quality shards passed. Attempt 4 is the latest result.

The exact-main trusted executor retry
[41068033905, attempt 3](https://github.com/zbarno/kool.ade/actions/runs/41068033905/attempts/3)
passed on code commit `713afd27`, including browser and desktop rootless
candidate checks, published-image smokes, and digest verification. The only
change from that code commit to current `main` is the documentation update in
PR #433. CodeQL passed on the corresponding PR heads before merge. At that
timestamp, the only change from the code commit to `main` was PR #433's
documentation update. Both QA profiles remain disabled; these results do not
authorize enabling them or certify the complete release matrix.

## Status after PR #434 merge (2026-10-30, 19:41 UTC)

PR #434 merged as [`24291aa9`](https://github.com/zbarno/kool.ade/commit/24291aa9a7620a30615c2ef1c6c1c586cd354602),
advancing `main` beyond `52504d90`. Exact-main quality
[41115005725](https://github.com/zbarno/kool.ade/actions/runs/41115005725)
and CodeQL [41115005003](https://github.com/zbarno/kool.ade/actions/runs/41115005003)
passed on the merge commit. Native packages
[41115005634](https://github.com/zbarno/kool.ade/actions/runs/41115005634)
and trusted executor images
[41115005668](https://github.com/zbarno/kool.ade/actions/runs/41115005668)
were still running at this snapshot. PR #434's candidate checks passed rootless
browser and desktop verification and reviewed-image digest checks before merge.
Profiles remain disabled, so candidate success does not authorize publication.

The remaining open PRs had not yet incorporated `24291aa9`:

| PR | Head and base | Check status at this snapshot |
| --- | --- | --- |
| #435 | [`99e148b`](https://github.com/zbarno/kool.ade/commit/99e148b94819ed4f5d3c4f2928d603a893a1c8fa) on base `b7297032` | Local fmt, nextest (3,663 passed, 17 skipped), doc tests, and Clippy passed. Hosted quality failed one test, `pursued_self_improvement_uses_current_app_plan_review_before_task_generation`, after an IO error reported a missing lifecycle-feature record; the same test passed in exact-main quality run [41115005725](https://github.com/zbarno/kool.ade/actions/runs/41115005725). Rootless browser and desktop candidate jobs and several native package jobs remained in progress. |
| #436 | [`359a365`](https://github.com/zbarno/kool.ade/commit/359a365ad6cffceeb93299983e5fe3e17e5d2f47) on base `b7297032` | All reported quality, CodeQL, trusted-image, and native-package checks passed; profile publication and smokes were skipped because both profiles are disabled. These checks do not validate the newer `24291aa9` base. |

The pre-merge checks below remain historical evidence for their listed heads.
Three technical #171 acceptance items remain open. Deferred human prelaunch
review does not block issue closure once technical acceptance is complete.

## Merged candidate PR checks (2026-10-30)

These hosted results passed on PR heads before merge. They provide candidate
implementation evidence; exact-main evidence is listed separately above:

| PR head | Quality | Trusted image | Native packages | Scope |
| --- | --- | --- | --- | --- |
| [#424 `1b19f33`](https://github.com/zbarno/kool.ade/pull/424/commits/1b19f333a896b461eee03a290365515c212aa217) | [40949815265](https://github.com/zbarno/kool.ade/actions/runs/40949815265) | [40949815276](https://github.com/zbarno/kool.ade/actions/runs/40949815276) | [40949815652](https://github.com/zbarno/kool.ade/actions/runs/40949815652) | Browser credential-bearing relay URL is rejected before Chromium starts. |
| [#426 `3364ca7`](https://github.com/zbarno/kool.ade/pull/426/commits/3364ca7fcdaee9483c093aed7d89589d9d29103e) | [40953781074](https://github.com/zbarno/kool.ade/actions/runs/40953781074) | [40953780987](https://github.com/zbarno/kool.ade/actions/runs/40953780987) | [40953780979](https://github.com/zbarno/kool.ade/actions/runs/40953780979) | Rootless desktop cancellation reports cancellation and removes its owned container. |
| [#429 `05df0a2`](https://github.com/zbarno/kool.ade/pull/429/commits/05df0a2b1c255c59ba5a29e5ed30be2d95d9bf27) | [40991463207](https://github.com/zbarno/kool.ade/actions/runs/40991463207) | [40991463145, attempt 3](https://github.com/zbarno/kool.ade/actions/runs/40991463145) | [40991463137](https://github.com/zbarno/kool.ade/actions/runs/40991463137) | Rootless desktop orphan recovery and candidate validation passed; merged as `c8623fb6`. |

PRs #424, #425, #426, and #429 are merged. PR #425's head contains
documentation only, so its PR-head checks are not listed as new runtime
candidate proof. PR #429's checks are candidate evidence; the exact-main run
above remains authoritative after merge. Trusted-image publication and
published-image smokes are skipped on `main` while both QA profiles are
disabled. These checks do not certify release readiness.

## Merged PR candidate checks (2026-10-30)

These hosted checks passed on PR heads before merge. They are candidate
implementation evidence and do not enable either QA profile:

| PR head | Quality | Trusted image | Native packages | CodeQL | Scope and remaining status |
| --- | --- | --- | --- | --- | --- |
| [#428 `8f3bb07`](https://github.com/zbarno/kool.ade/commit/8f3bb07f41c32241359a6b52f401096dfe9dbcd5) | [41059042295](https://github.com/zbarno/kool.ade/actions/runs/41059042295) passed | [41059042298](https://github.com/zbarno/kool.ade/actions/runs/41059042298) passed: browser and desktop candidates, published-image smokes, and digest verification | [41059042355](https://github.com/zbarno/kool.ade/actions/runs/41059042355) passed | [41059036492](https://github.com/zbarno/kool.ade/actions/runs/41059036492) passed | Adds rootless host-HOME, Docker/display/service socket, environment, and `noexec` executable-denial probes. The 300-second desktop transport watchdog keeps the driver's approved per-run timeout authoritative. Earlier browser Docker health disconnects on `15ca529` did not recur. Merged as `713afd27`; technical #171 criteria remain open. |

PR #428 merged as `713afd27`. Its candidate quality, CodeQL, native package,
browser and desktop rootless, and published-image checks passed. The exact-main
trusted executor retry on that code commit passed at [41068033905, attempt 3](https://github.com/zbarno/kool.ade/actions/runs/41068033905/attempts/3).
Three technical #171 acceptance items remain open. Deferred human prelaunch
review does not block issue closure after technical acceptance is met.

## Previous exact-main matrix (2026-10-27)

Evidence is pinned to exact `main` commit
[`2e58d2b`](https://github.com/zbarno/kool.ade/commit/2e58d2b75139b164903c5a09858b384fbfa991d9).
The candidate workflow is [run 40600683463](https://github.com/zbarno/kool.ade/actions/runs/40600683463);
the required quality workflow is [run 40600683473](https://github.com/zbarno/kool.ade/actions/runs/40600683473),
which passed on attempt 2 after attempt 1 failed during isolated Docker daemon recovery.
The trusted-image run passed rootless browser and desktop candidate suites,
including app-owned browser preview/driver evidence and the approved-task
Build/Test/Start-to-browser-artifact path. Both QA profiles in
[`containers/images.lock.json`](../containers/images.lock.json) still have
`runtime_enabled: false`. Publication and published-image smoke jobs were
skipped, so candidate success is not a release authorization.

“Partial” records only the evidence proved by the linked run; it does not
certify the full row. The live issue body currently marks its first of five
release-acceptance boxes checked, while its introduction says zero are
certified. This matrix rates that first criterion **Partial** because the
profiles remain disabled and the cited run does not prove the full release
acceptance path. Reconcile the tracker checkbox with reviewable release proof.

| #171 requirement | Exact-main evidence | Status and remaining acceptance |
| --- | --- | --- |
| 1. Docker RuntimeQa security matrix across local/remote Linux | Rootless browser and desktop candidate suites passed in [trusted-image run 40600683463](https://github.com/zbarno/kool.ade/actions/runs/40600683463). The workflow verifies a rootless daemon; browser scenarios include relay isolation and origin/network-denial coverage. | **Partial.** The complete local/remote matrix for credentials, host resources, mounts, roles, network boundaries, and service-port relay is not evidenced. |
| 2. Pinned rootless Linux desktop and browser QA images | Browser `linux/amd64` is locked to `sha256:a247709202b9af7244392e0fa24268bfb9279afed820d69a18a7261a0ea8c46e`; desktop `linux/amd64` is locked to `sha256:541efb7b83f0c55f81533058fe02a7b68e1136eeb2324d21e968e9e93eb30fb6`. Candidate builds and verification passed in [run 40600683463](https://github.com/zbarno/kool.ade/actions/runs/40600683463); both profiles in [`containers/images.lock.json`](../containers/images.lock.json) remain disabled. | **Partial.** Publication and published-image smoke were skipped. No release approval or enabled profile exists. |
| 3. Real desktop and browser fixtures detect seeded defects | In [run 40600683463](https://github.com/zbarno/kool.ade/actions/runs/40600683463), browser tests `rootless_app_owned_browser_preview_and_driver_produce_evidence_and_clean_up` and `rootless_approved_task_build_test_start_receipts_feed_browser_qa_artifact` passed; desktop test `rootless_app_owned_qa_driver_uploads_runs_reads_and_cleans_candidate` passed. | **Partial.** These tests do not establish the full desktop/browser interaction and seeded-defect matrix (including all resize, clipping, crash/hang, responsive, console, and disabled-control cases). |
| 4. Approved-plan-to-PR lifecycle repair loop | Deterministic lifecycle and evidence tests are included in [quality run 40600683473, attempt 2](https://github.com/zbarno/kool.ade/actions/runs/40600683473), which passed after attempt 1 failed during isolated Docker daemon recovery. | **Partial.** The run does not prove the complete approved-plan, implementation, independent review, QA repair, spec reconciliation, and distinct PR work-item path against the candidate images. |
| 5. Negative runtime security tests | Rootless browser candidate coverage and relay-token/origin-escape checks passed on the reviewed main state in [run 40600683463](https://github.com/zbarno/kool.ade/actions/runs/40600683463). | **Partial.** The complete runtime-denial battery for host credentials/HOME, Docker socket, host display/DBus, arbitrary executables, installs, internet/metadata, forged evidence, and shell escalation is not evidenced. |
| 6. Runtime failure and recovery matrix | [Quality run 40600683473, attempt 2](https://github.com/zbarno/kool.ade/actions/runs/40600683473) passed Docker daemon recovery and queue-concurrency checks after attempt 1 failed during isolated Docker daemon recovery. | **Partial.** It does not evidence every QA-specific daemon, driver, preview, storage, cancellation, remote-worker, and restart failure case. |
| 7. Reviewer prompt evaluations | Prompt construction and evidence-bound review behavior have deterministic tests in the quality workflow. | **Not started for release acceptance.** No measurable model-recall evaluation set and reviewed result for the required adversarial and visual-regression prompts is attached. |
| 8. Setup diagnostics and Task Details evidence | Current implementation and operator guidance are documented in [`runtime-qa-operations.md`](runtime-qa-operations.md) and exercised by the quality workflow. | **Partial.** Full setup/readiness and repair-timeline UX acceptance, evidence quota/retention behavior, and manual support review remain outstanding. |
| 9. Architecture, runbook, privacy, recovery, and support docs | Current contracts and operations guidance are linked from [`runtime-qa-operations.md`](runtime-qa-operations.md), [`runtime-qa-contract.md`](runtime-qa-contract.md), and [`project-runtime-contract.md`](project-runtime-contract.md). | **Partial.** Documentation exists; measured startup/performance and live manual smoke evidence remain outstanding. |
| 10. Required CI and release checklist gate | The required format, tests, docs, Clippy, Docker runtime, recovery, and concurrency checks passed in [quality run 40600683473, attempt 2](https://github.com/zbarno/kool.ade/actions/runs/40600683473). Candidate image suites passed separately in [run 40600683463](https://github.com/zbarno/kool.ade/actions/runs/40600683463). | **Partial.** Candidate QA suites are not included in the `quality` aggregator's required needs, and publication/smoke jobs were skipped while profiles remain disabled. |

The cited rootless CI jobs are hosted-only on this machine. No local test result
is used as a substitute for them. Future updates must record the exact commit,
workflow run or manual smoke artifact, image digest, scenario, and observed
result. A passing source test or candidate-image run changes only the row it
directly covers.

## Historical evidence snapshot (2026-10-23)

The dated table below is a **historical evidence snapshot for PR #264**, not
the current `main` release sign-off. #264 has now **merged**, as have #295
(published-image digest smoke testing) and #305 (block false screenshot-free
passes on mixed/visual targets). Open PR #298 proposes more report-ordering,
redaction and source-bound evidence hardening. These implementations do not
prove the full approved task-to-build-to-independent browser/desktop QA run
on production-enabled pinned images; both image profiles remain
`runtime_enabled: false`. Visual requirements without independently inspected
screenshot pixels must remain **untested**. The release gate #171 stays open
until updated exact-candidate CI/security/UX/manual proof is recorded.


Evidence snapshot: 2026-10-21 for PR head
[`930f1a6`](https://github.com/zbarno/kool.ade/commit/930f1a64a23edd5830b65eba4a55103bffd77c0c)
on PR [#264](https://github.com/zbarno/kool.ade/pull/264); candidate image provenance
binds to merge revision
[`ac540ac`](https://github.com/zbarno/kool.ade/commit/ac540acf1dc5f2b9b3cdd7570171f6441570911c).
This table tracks the
mandatory release matrix in issue [#171](https://github.com/zbarno/kool.ade/issues/171).
“Partial” means the cited evidence proves only the listed slice. Candidate image
tests and local lifecycle tests do not satisfy the end-to-end release gate.

This is a historical evidence snapshot for the cited PR #264 run. Its candidate
digests are not the image references currently recorded in
`containers/images.lock.json` after PR #264 merged, and they do not validate
candidate recipe changes in this checkout. The current QA profiles remain
disabled; regenerate rootless candidate evidence and reconcile the lock before
using any candidate digest as current release evidence.

| #171 requirement | Evidence on this revision | Status and remaining acceptance |
| --- | --- | --- |
| 1. Docker RuntimeQa security matrix across local/remote Linux | [Trusted image run 39522095369](https://github.com/zbarno/kool.ade/actions/runs/39522095369) rebuilt both rootless QA candidates and verified the reviewed digests. The browser candidate job exercised metadata redirect/fetch/iframe, external WebSocket, localhost, alternate-port denial, host TCP/UDP and WebRTC escape probes. | **Partial.** No complete RuntimeQa security matrix covering remote workers, credentials, every host resource, full role parity, and all network/filesystem boundaries. |
| 2. Pinned rootless Linux desktop and browser QA images | [Trusted image run 39522095369](https://github.com/zbarno/kool.ade/actions/runs/39522095369) passed rootless `browser_qa` and `desktop_qa` candidate builds, app-owned lifecycle exercises, and exact reviewed-lock verification on PR head `930f1a6`. Candidate digests are browser `sha256:cde309ff57c1c8454b18d06b4c6e3165df2184d0caafd0ff06d37ef6e9675a83` and desktop `sha256:5240b227566d9b69187420972bc0941be6e12adf940ef867ff6237defbb13041`; their provenance source revision is `ac540acf1dc5f2b9b3cdd7570171f6441570911c`. | **Partial.** Publication jobs were skipped; both profiles remain `runtime_enabled: false`. Trusted published images and release approval are absent. |
| 3. Real desktop and browser fixtures detect seeded defects | In [trusted image run 39522095369](https://github.com/zbarno/kool.ade/actions/runs/39522095369), the desktop candidate job ran the native egui and packaged Chromium fixtures through mouse/keyboard interaction, screenshot evidence, foreign-window exclusion, and seeded failure detection. The browser candidate job ran Playwright against the isolated preview fixture and exercised viewport, console, origin-policy, and seeded defect cases. | **Partial.** The complete matrix (resize, clipping, crash/hang, responsive browser states, console exceptions, disabled controls, and defect detection for each) is not yet accepted as a release suite. |
| 4. Approved-plan-to-PR lifecycle repair loop | [Quality run 39522095404](https://github.com/zbarno/kool.ade/actions/runs/39522095404) passed `qa_finding_repairs_new_code_repasses_and_reconciles_once_to_pr_work_item`, a deterministic in-process lifecycle acceptance test. Scenario generation and persistence also revalidate current lifecycle task/spec state, approved recipes, and successful build receipts. | **Partial.** This does not run the full lifecycle against the pinned browser and desktop images or prove the complete application interaction and PR path in CI. |
| 5. Negative runtime security tests | [Trusted image run 39522095369](https://github.com/zbarno/kool.ade/actions/runs/39522095369) includes rootless browser origin and host/private network escape denial; the app-owned browser job also verifies preview, browser run, evidence, and cleanup. | **Partial.** Remaining runtime-denial attempts include host credentials/HOME, Docker socket, host display/DBus, arbitrary executables, package installs, internet/cloud metadata variants, forged lifecycle evidence, and shell escalation. |
| 6. Runtime failure and recovery matrix | [Quality run 39338374505](https://github.com/zbarno/kool.ade/actions/runs/39338374505) passed Docker runtime checks. One existing recovery integration test timed out on its first attempt and passed on retry (attempt 2). | **Partial.** The complete QA-specific failure matrix (daemon loss/restart, missing tools, driver/preview crash, full storage, cancellation, remote worker loss, concurrency, and restart recovery) is not yet evidenced. |
| 7. Reviewer prompt evaluations | Prompt versions and evidence-bound reviewer records are implemented and covered by deterministic tests in the quality workflow. | **Partial.** No measurable model-recall evaluation set covering the listed hidden ambiguity, malicious DOM instructions, overengineering, failed acceptance, and visual regression cases is attached to this release record. |
| 8. Setup diagnostics and Task Details evidence | Settings currently shows Docker/image/reviewer/relay readiness, evidence-folder readability and free space, the per-run 2 GiB cap, and the no-expiry policy. Task Details projects validated screenshots, traces, coverage, and findings. | **Partial.** There is no global evidence quota or automatic expiry; manual retention and storage management remain limited. The setup and repair-timeline UI still needs full acceptance review. |
| 9. Architecture, runbook, privacy, recovery, and support docs | `docs/runtime-qa-contract.md`, `docs/project-runtime-contract.md`, the [operator guide](runtime-qa-operations.md), and the README roadmap document current boundaries, readiness checks, evidence review, recovery limits, post-release smoke steps, and unsupported environments. | **Partial.** The operator guide is source-checked, but performance/startup measurements and live manual smoke evidence remain outstanding. |
| 10. Required CI and release checklist gate | On exact PR head `930f1a6`, [quality run 39522095404](https://github.com/zbarno/kool.ade/actions/runs/39522095404) passed format, tests, Clippy, docs, Docker runtime, and Windows discovery checks. [Trusted image run 39522095369](https://github.com/zbarno/kool.ade/actions/runs/39522095369) passed candidate verification and reviewed-lock checks. | **Partial.** The full QA security/lifecycle matrix is not yet a required release gate. Runtime QA remains unavailable and image publication remains disabled. |

## Acceptance rule

Do not enable either QA image profile or describe Runtime QA as shipped until
every applicable release row has a passing, reviewable artifact for the
supported platform matrix and release approval is recorded. Candidate delivery
and passing candidate suites are not release approval. Close implementation
issues #166, #167, #168, #169, and #173 once their own technical acceptance is
complete; deferred human prelaunch review does not keep a technically complete
issue open. Keep #171 open while technical release acceptance remains. For each
update, record the exact commit, workflow run or manual smoke artifact, image
digest, fixture/scenario, and observed failure result. A passing source test or
candidate-image smoke only changes the row it directly covers.
