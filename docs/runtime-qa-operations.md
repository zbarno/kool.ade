# Runtime QA operator guide

## Availability

Production Runtime QA remains **Unavailable**. Settings > Execution
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

## Local experimental dogfooding

Settings > Execution Environments > Experimental Features exposes implemented
Browser QA and Linux desktop QA independently from production image policy.
The machine-wide switch defaults off and requires a local confirmation. Its
settings live in the user's application-data directory, outside code and
planning repositories, with user-only file permissions. The registry continues
to show blocked and not-implemented capabilities when the switch is off.
Computer Use is a separate experimental local QEMU/KVM capability with its own
host readiness and release gate; it is not a Runtime QA driver. Wiring Computer
Use into the Runtime QA workflow remains tracked by #389 and is not claimed as
available here. See the [Computer Use architecture and operator guide](computer-use-architecture.md).

On Linux amd64, the app selects only the exact immutable QA digest and recipe
recorded in `containers/images.lock.json`, and only while that digest is
already present on a ready rootless Engine. To stage the currently pinned
images, an operator can explicitly pull the exact references currently listed
in `containers/images.lock.json`:

```sh
docker pull --platform linux/amd64 ghcr.io/zbarno/koolade-runtime-qa-browser@sha256:a247709202b9af7244392e0fa24268bfb9279afed820d69a18a7261a0ea8c46e
docker pull --platform linux/amd64 ghcr.io/zbarno/koolade-runtime-qa-desktop@sha256:541efb7b83f0c55f81533058fe02a7b68e1136eeb2324d21e968e9e93eb30fb6
```

Dispatch never downloads an image. The local
`scripts/build-qa-image-candidate.sh` workflow builds and smoke-tests candidates
from a digest-pinned base with isolated BuildKit and fixed synthetic inputs; its
mutable candidate tag is not selectable unless it matches a locked digest.
Mutable tags, arbitrary local image names, and rootful or host-execution
fallbacks are not accepted. The lock's `runtime_enabled: false` values remain
unchanged.

Enable the machine-wide option, then enable only the QA features to exercise.
“Enable all testable experimental features” sets those opt-ins but does not
change platform, Engine, image, reviewer, recipe, build, readiness, scenario,
or screenshot checks. Follow each displayed blocker. After independent code
review and a successful build, open the task's Runtime QA scenarios, inspect
the exact synthesized cases and redaction, then choose **Approve scenarios and
run experimental QA**. The existing dispatcher rechecks the request and runs
the existing isolated QA worker.

Every experimental attempt stores its mode and exact image digest with its
task-bound evidence. A passing experimental result remains visible in Task
Details as local experimental evidence; it records the attempt as blocked for
the production lifecycle and cannot create a production gate receipt, satisfy
release certification, or authorize publication. Turning the mode off blocks
new experimental dispatch. It does not forcibly stop an already owned worker;
that isolated run continues under its captured request until it finishes or the
app/runtime lifecycle interrupts it. Its evidence and cleanup ownership remain
unchanged. Re-enabling requires fresh task/request checks.

This local workflow is not the deferred whole-system acceptance below. Browser
and desktop interaction, pixel review, persisted pass/failure behavior, and
live-container denial of HOME/SSH, Docker socket, host display, ambient network,
and arbitrary commands still require the pinned rootless fixtures to run on
the exact integrated candidate. Record those scenarios only after they have
actually run; retain #171's production release gate in the meantime.

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
