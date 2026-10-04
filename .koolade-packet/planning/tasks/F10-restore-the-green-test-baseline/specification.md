# F10: Restore the green test baseline
<!-- koolade-artifact-id:v1 {"uid":"975b7c28-2565-42d2-9276-b101df28c928","displayId":"F10","title":"Restore the green test baseline"} -->
<!-- koolade-change:v1 {"schemaVersion":1,"uid":"975b7c28-2565-42d2-9276-b101df28c928","displayId":"F10","status":"abandoned"} -->

**Generation-time status:** Ready (superseded by the abandoned status below).

**Status:** Abandoned — this task-generation copy preserves the original proposal; its failure attribution and gate description are historical and unverified.

## Historical proposal

At filing, this proposal was to investigate a reported set of 49 failures at base 557240e and restore a green test gate. F10 was not implemented to completion, and the report did not classify those failures individually. The proposal and its worker attribution are historical claims, not a current gate report or verified diagnosis.

## Reported behavior at filing (unverified historical record)

- The filing record reported 728 passed / 49 failed (exit 101) at base 557240e and a matching sorted failure-name fingerprint on the then-pristine base and F9 worktree. The historical report was not independently reclassified as part of F10.
- The F9 implementation report attributed failure families to worker-sandbox fixture seeding and temporary-directory idempotency. That attribution was not verified per test and remains unconfirmed.
- The filing record described the test gate as red in its sandbox at that time. This is not the current public-snapshot gate result; current validation is recorded separately.

## Desired Behavior

- `cargo test --locked --all-targets -- --test-threads=1` at the base exits 0 with zero failures.
- Every one of the 49 previously failing tests is individually accounted for in an implementation report: fixed in place, or stabilized/exempted with a written, evidenced reason.
- Fixes favor fixture and test-plumbing hygiene (isolated unique scratch locations, deterministic seeding, reliable cleanup) over weakening assertions and over production-behavior changes.
- After the repair, the held F9 two-line commit and later queued deliveries pass the unmodified gate.

## Scope

- Included: reproducing the 49-name failure set at the base; per-test classification (environment-bound versus potential true regression); remediation of environment-bound failures; full gate reruns (fmt, test, clippy); reporting with evidence.
- Excluded: the F9 relabel itself; relaxing the mandated gate rule (separate operator decision); repository renaming (the public-launch decision keeps `zbarno/kool.ade`); any product-behavior redesign.

## Affected Product Areas

- `product:quality-and-acceptance` — gate-health statements reconcile after the repair lands.
- F9 — downstream beneficiary: its commit gate clears.

## Requirements

1. R1 — Reproduce the full failure set at base 557240e and record the 49 distinct test names plus representative log excerpts in the implementation report.
2. R2 — Classify each failing test as environment-bound (fixture seeding, temp-dir/idempotency, sandbox limitation) or a potential true regression, with per-test evidence.
3. R3 — Remediate every environment-bound failure by hardening test scaffolding (unique isolated scratch locations, deterministic seeding, reliable cleanup); no weakening of assertions.
4. R4 — Surface any potential true regression as separate review/findings items with minimal reproduction; do not silently patch them inside this feature.
5. R5 — After the repair, fmt, clippy (--all-targets with -D warnings), and the full base test suite all pass; attach log evidence to the report.
6. R6 — Prefer test-harness changes over production-behavior changes; itemize any unavoidable behavior change with its justification in the report.

## Decisions and Assumptions

- Historical trigger: an operator query was interpreted as a request to investigate. The task batch was archived incomplete; this task-generation copy does not record an active approval or worker request.
- Historical working assumption: the original planning card proposed that all 49 failures were environment-bound. This was not verified, is superseded by F10's abandoned status, and must not be treated as a finding.
- Historical sequencing (CLR-009): the original plan described repair-first. It was superseded by the later F9 outcome, and F10 did not execute either repair or per-test classification.
- Pinned toolchain as used by the F9 gate scripts: cargo +1.98.1, --locked, single-threaded test runs.

## Acceptance Criteria

- AC1 — `cargo test --locked --all-targets -- --test-threads=1` at the base exits 0 with 0 failed (log excerpt recorded).
- AC2 — The report maps all 49 previously failing test names to fixed-in-place or exempted-with-justification outcomes.
- AC3 — fmt and clippy are green; the remediation diff is test-harness-heavy, and any behavior change is itemized per R6.
- AC4 — Downstream proof: the held F9 commit passes all three gates without exception under either ordering (prerequisite to landing if repair-first; logged afterward if land-first).
