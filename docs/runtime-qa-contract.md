# Runtime QA records and gate

Runtime QA records are versioned and bound to one task, approved specification,
source SHA, execution session, platform, and bounded driver profile. A request
names every approved requirement and acceptance criterion. Scenario sets must
cover every requirement and stay within the requested scenario/action limits.

The evidence manifest records the exact run and source identity, driver and
build hashes, platform/display metadata, action trace, screenshots, assertions,
logs, crashes, browser diagnostics, and retention policy. Screenshot paths are
run-scoped. Findings cite reproducible steps and available evidence. A result
cannot pass with skipped or blocked cases, unobserved requirements, blocking
findings, missing screenshots, or failed/untested assertions. The result must
also come from a reviewer run and session distinct from the implementation run
and session.

Approved screenshot redaction regions are part of the scenario and therefore
part of its approval identity. The shared report assembler validates screenshot
bytes and dimensions, applies opaque pixel masks before storage, and records the
mask geometry and hash of the transformed PNG. This covers browser and desktop
drivers because both reports pass through that boundary. A scenario without
approved regions remains unredacted and the scenario editor warns about that;
reviewers must not assume automatic detection of personal or secret data.

`RuntimeQaStore` persists request, scenario, evidence, result, screenshots, and
finding disposition records as collision-safe immutable files below the
current user's application data directory, scoped by workspace ID. It does not
write screenshots, logs, or reviewer output into a project checkout or planning
Git repository. Linux uses `$XDG_DATA_HOME` or `$HOME/.local/share`; macOS uses
`~/Library/Application Support`; Windows uses `%LOCALAPPDATA%`. If the local
data directory cannot be resolved, QA stops with an explicit storage error.
Older development records under an embedded `.koolade-packet/state/runtime-qa`
directory are left intact and are not automatically imported.

`approved_gate_receipt` converts a validated report into a lifecycle receipt
bound to the task, source SHA, exact current Runtime QA reviewer prompt,
reviewer/author identities, and evidence references. Non-pass outcomes request
changes; they do not approve the task.

`containers/runtime-qa/browser/driver.mjs` is the first concrete platform
driver. It launches an ephemeral Playwright/Chromium context, executes only
typed browser actions, binds navigations and subresources to one explicit local
preview relay origin, and emits action observations, screenshot bytes and hashes,
console errors, and failed requests. The Python fixture at
`tests/fixtures/runtime_qa/browser/server.py` supplies a non-Node target and
seeded functional/visual failures.

The verified MCR base digests and offline derived-image recipes are in the
rootless image workflow. CI builds and exercises candidate browser and desktop
drivers, including real interactions against synthetic apps. Their reviewed
candidate digests are recorded in `containers/images.lock.json`, with both QA
profiles still marked `runtime_enabled: false`. Runtime selection therefore
continues to fail closed until trusted publication and the separate #171
release gate are reviewed. This prevents a candidate image or synthetic
fixture from being treated as a released QA capability. The source contains
the isolated Docker session, target-recipe checks, lifecycle queue, Manager
finding routing, and desktop/browser drivers; the complete application-level
implementation-to-QA-to-repair-to-spec-sync path still needs an end-to-end CI
run against the pinned images before Runtime QA can be called release-ready.

The desktop image candidate smoke runs the packaged Chromium fixture inside the
candidate image under private Xvfb, executes pointer and keyboard actions on a
synthetic loopback page, resizes the window, and verifies distinct screenshot
hashes plus the expected state changes. This crosses the image-to-driver,
process-isolation, and screenshot-evidence boundary; a unit test cannot prove
that the pinned browser launches with its packaged display dependencies or
that real input changes rendered output inside the container. It requires a
rootless Linux Docker daemon and Buildx, uses no external network at runtime,
and has high setup cost because it builds the candidate image before the smoke.
It does not exercise the full task approval, recipe execution, QA repair, and
spec reconciliation lifecycle.
The same candidate smoke also opens an actual eframe/egui window under private
Xvfb, applies pointer and keyboard input through the desktop driver, checks the
updated native window state, and retains the resulting screenshots. This
crosses the egui window and input boundary that the direct X11 and Chromium
fixtures do not cover. It uses the same rootless Docker setup and adds one
bounded window launch to the image smoke.

Approved desktop scenarios may address `active` when an action needs a window
ID. The driver resolves it only when exactly one visible window belongs to the
launched target process; zero or multiple owned windows fail closed. This lets
scenarios synthesized before launch perform real pointer interaction without
guessing a transient X11 ID. Window-action post-state records the resolved
numeric ID; the redacted arguments retain the submitted `active` selector.

Task Details projects the persisted run outcome, requirement coverage, findings,
action trace, and validated screenshots from the user's local Runtime QA store.
The projection checks the run request and result hash against the lifecycle
task receipt before displaying it. Missing or corrupt evidence is reported as
unavailable; it is never presented as a pass. This UI is an evidence browser,
not proof that the app interaction happened on the current machine.

The `qa_finding_repairs_new_code_repasses_and_reconciles_once_to_pr_work_item`
acceptance test exercises the production Runtime QA orchestrator and lifecycle
stores through failure, repair, fresh review, pass, and spec reconciliation
using deterministic in-process driver and reviewer adapters. It covers the
orchestrator-to-lifecycle persistence boundary and stale-evidence invariant;
a unit test cannot establish that the durable records compose through the full
transition sequence. It has no Docker, browser, model, or network dependency
and negligible setup cost. The screenshot assertion in
`immutable_approved_run_persists_report_and_returns_gate_receipt` also exercises
the orchestrator-to-storage redaction boundary: it proves masked pixels are
persisted and the stored hash is computed from those bytes. A helper unit test
alone cannot prove that the real report assembly persists the transformed
image. It has no external dependencies and negligible additional setup cost.
The lifecycle acceptance test does not satisfy the pinned-image application
E2E requirement: real desktop/browser interaction, private screenshot
collection, release security negatives, and setup UX remain separately
required.
