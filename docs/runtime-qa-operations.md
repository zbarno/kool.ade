# Runtime QA operator guide

## Availability

As of 2026-10-20, Runtime QA is **Unavailable**. Settings > Execution
Environments > Runtime QA reports image policy, local image presence, reviewer
route, preview relay, and evidence storage readiness. A ready Docker engine or
locally present candidate image does not enable QA. The `browser_qa` and
`desktop_qa` entries in `containers/images.lock.json` must remain
`runtime_enabled: false` until the #171 release gate is reviewed and trusted
publication is authorized.

The **Trusted executor images** workflow builds browser and desktop candidates
on a rootless Linux Docker daemon. Its retained artifacts include the candidate
manifest and per-image build evidence; fixture pass/failure is visible in the
workflow logs, and desktop screenshot evidence is retained with the candidate.
For QA profiles, verify the workflow's rootless setup and image-lock
verification jobs passed; a local rootful Docker run is not equivalent
evidence. Candidate builds exercise synthetic applications and do not run an
arbitrary project Dockerfile.

## Readiness checks

Before investigating a blocked run, record the task ID, target ID, source SHA,
QA run ID (if one was created), and the exact status shown in Task Details.
Check these items in order:

1. **Target and recipes:** Task Details must show the selected target for that
   task and source. Build, Test, Start, and readiness evidence are separate.
   An approved recipe is not a passing receipt. Missing tools or dependencies
   remain blocked until an authorized image or preparation is available.
2. **QA image policy:** In Runtime QA settings, check both profile policies and
   digests. `disabled by reviewed image policy` is an intentional release gate,
   not a local Docker problem. Do not edit the lock to enable a profile.
3. **Reviewer route:** In Coding Tools, refresh the selected QA route's
   capability probe. The Runtime QA page must show a fresh reviewer capability.
4. **Preview relay:** The relay is created for an individual run. It is not a
   general host port-forward and must not be replaced with a host-published
   service port.
5. **Evidence storage:** Settings reports the user-local Runtime QA directory
   and available disk space. On Linux this is
   `$XDG_DATA_HOME/koolade/runtime-qa`, or
   `~/.local/share/koolade/runtime-qa` when `XDG_DATA_HOME` is unset. On macOS
   it is `~/Library/Application Support/koolade/runtime-qa`; on Windows it is
   `%LOCALAPPDATA%/koolade/runtime-qa`. Evidence is immutable and scoped by
   workspace. Each approved request can retain up to 2 GiB; automatic expiry
   and a global quota are not implemented. Check free disk space before
   retrying a large run.

## Reviewing a result

Open the task's Runtime QA section in Task Details. Confirm that the displayed
run and result hashes match the lifecycle receipt, then inspect requirement
coverage, scenario outcomes, action trace, findings, and screenshots. Missing
or corrupt evidence is unavailable evidence; it must not be treated as a pass.
For a failure, use the recorded scenario and observation to make a repair
proposal. A code change creates a new source SHA and requires fresh code review
and QA. The previous result remains historical evidence for its original SHA.

For a headless service, a `NotApplicable` visual-QA decision is not an API or
integration-test pass. Review the separate source-bound Build and Test receipts
and the applicability decision. Service-specific QA evidence is still an open
acceptance gap.

## Recovery and cleanup

- If QA is unavailable because a profile is disabled, stop there and retain the
  diagnostic. Do not rerun with a host browser, desktop session, or rootful
  Docker daemon.
- If evidence storage is unreadable or low on space, preserve existing records
  and collect the storage path, free-space reading, and run ID. Do not delete
  workspace evidence as a recovery shortcut; no supported retention/expiry
  workflow currently exists.
- If a run reports a missing or changed target, recipe, readiness probe, or
  artifact allowlist, re-review the current Task Details approval and create a
  new run from the current source. Do not reuse the stale request.
- If a container or driver fails, preserve the run result and owned-resource
  cleanup status. Retry only after the cause is understood; a successful retry
  does not erase the failed attempt.
- For a CI candidate failure, retain the workflow URL and artifact names. A
  candidate failure is not repaired by manually pulling or retagging an image.

## Manual acceptance checklist after release approval

These are release verification steps, not currently available user operations.
Run them only against a reviewed, runtime-enabled image revision and a synthetic
fixture project:

1. From a clean task clone, approve a target and its recipes, build it, and
   confirm source-bound Build/Test/Start receipts appear separately.
2. Start independent QA from the approved task and frozen acceptance criteria.
   Confirm the reviewer cannot edit source, install packages, access user
   credentials, or select arbitrary commands.
3. Exercise a passing scenario and a seeded failure. Confirm screenshots,
   action trace, coverage, and finding hashes are retained and displayed for
   the exact source SHA.
4. Repair the seeded failure. Confirm the new SHA invalidates old review/QA
   approval, then run fresh review and QA before spec reconciliation and Done.
5. Cancel a run and simulate a driver or preview failure. Confirm the task is
   not passed and cleanup removes only resources owned by that run.
6. Run two independent tasks concurrently and confirm their preview ports,
   evidence, and cleanup remain isolated.

Record the commit, trusted image digest, workflow run, scenario, and observed
result in `runtime-qa-release-evidence.md`. Do not describe the feature as
shipped until every #171 row is supported by reviewable evidence.

## Support boundary

The reviewed candidates currently cover Web and Linux desktop on Linux amd64.
This does not establish support for remote GUI workers, other operating
systems, native mobile, embedded devices, or GPU-specific rendering. The
software-rendered desktop path does not prove hardware-rendering behavior.
Unsupported surfaces must remain explicitly unavailable until their drivers
and release evidence are reviewed.
