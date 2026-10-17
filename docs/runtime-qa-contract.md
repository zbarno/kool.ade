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
drivers, including real interactions against synthetic apps. Runtime selection
still fails closed while reviewed derived-image digests are absent from
`containers/images.lock.json`. This prevents a candidate image or synthetic
fixture from being treated as a released QA capability. The source contains
the isolated Docker session, target-recipe checks, lifecycle queue, Manager
finding routing, and desktop/browser drivers; the complete application-level
implementation-to-QA-to-repair-to-spec-sync path still needs an end-to-end CI
run against the pinned images before Runtime QA can be called release-ready.

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
and negligible setup cost. It does not satisfy the pinned-image application
E2E requirement: real desktop/browser interaction, private screenshot
collection, release security negatives, and setup UX remain separately
required.
