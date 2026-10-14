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

`RuntimeQaStore` persists request, scenario, evidence, result, and finding
disposition records as collision-safe immutable JSON files. `approved_gate_receipt`
converts a validated report into a lifecycle receipt bound to the task, source
SHA, exact current Runtime QA reviewer prompt, reviewer/author identities, and
evidence references. Non-pass outcomes request changes; they do not approve the
task.

`containers/runtime-qa/browser/driver.mjs` is the first concrete platform
driver. It launches an ephemeral Playwright/Chromium context, executes only
typed browser actions, binds navigations and subresources to one explicit local
preview relay origin, and emits action observations, screenshot bytes and hashes,
console errors, and failed requests. The Python fixture at
`tests/fixtures/runtime_qa/browser/server.py` supplies a non-Node target and
seeded functional/visual failures.

The verified MCR base digest and offline derived-image recipe are in the image
lock/build workflow. Runtime selection deliberately fails closed because the
derived image release digest and app Docker session are not yet wired. Linux
desktop input and lifecycle Manager routing are also unavailable. The runtime
must report those surfaces unavailable; source code, a fixture, or a stored
report alone is not evidence that desktop or browser behavior was exercised.
Task-queue orchestration, test-data provisioning, screenshot collection into
user-local storage, repair routing, and the end-to-end gate lifecycle remain
release blockers.
