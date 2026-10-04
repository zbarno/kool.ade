---
<!-- koolade-artifact-id:v1 {"uid":"f27bc9f0-f995-4ad6-81fc-aeeac7f1f562","displayId":"F10-TASK-capture-and-classify-the-49-failure-baseline","title":"Capture and classify the 49-failure baseline","parentUid":"106cdc43-203a-42fb-8104-297c38b9b8c0"} -->

koolade-task: {"schemaVersion":1,"uid":"f27bc9f0-f995-4ad6-81fc-aeeac7f1f562","batchUid":"106cdc43-203a-42fb-8104-297c38b9b8c0","repositoryId":"root","dependencyUids":[]}
---

# F10-TASK-capture-and-classify-the-49-failure-baseline — Capture and classify the 49-failure baseline

Feature: Restore the green test baseline (F10)

Status: Individually validated; see batch index for generation status.

## Problem this ticket solves and why

The base suite is red with 49 failing tests but there is no consolidated ledger naming and classifying each one, so later repairs and the report cannot map dispositions. Establishing that ledger is the prerequisite every remediation task builds on.

## Ticket goal — what changes when done

A written ledger listing all 49 distinct failing test names with representative log excerpts, each tagged environment-bound or possible-regression with per-test evidence.

## User story

As the sole developer-operator I want each of the 49 red tests captured and classified so I can see which need fixture hardening and which might be real regressions before anything ships.

## Purpose

Run the pinned single-threaded suite at base 557240e, capture the 49 distinct failing test names plus representative logs, and classify each as environment-bound or a possible true regression with per-test evidence, establishing the ledger every later disposition maps to.

## Specification references

Source: [Approved specification](specification.md)

## Implementation context

Base 557240e measures 728 passed / 49 failed with a failure fingerprint identical to the F9 docs-only worktree, indicating pre-existing environment debt. Known families are swglue ws fixture seeding and artifact-migration /tmp temp-dir idempotency; confirmation is owed by this task.

Feature ID: F10
Repository: root

## Approved scope mapping

- Scope 1: Reproduce the full 49-name failure set at base 557240e and record the test names plus representative log excerpts (R1)

- Scope 2: Classify each failing test as environment-bound versus a potential true regression, with per-test evidence (R2)
- Success criterion 1: `cargo test --locked --all-targets -- --test-threads=1` at base 557240e exits 0 with 0 failed, with a log excerpt recorded (AC1)
- Success criterion 2: The report maps all 49 previously failing test names to fixed-in-place or exempted-with-justification outcomes (AC2)

## Dependencies

None. This task can start independently.

## Affected files and components

- Implementation report location under .koolade-packet/implementation/
- Test sources under src/ producing the failing tests
- Integration tests under tests/

## Implementation steps

1. Verify checkout is at base 557240e before running anything
2. Run the pinned single-threaded suite and capture full output
3. Extract the 49 distinct failed test names and sort/dedupe them
4. Attach a representative log excerpt to each name
5. Tag each as environment-bound or possible-regression with a per-test evidence line
6. Save the ledger in the implementation report area

## Acceptance criteria

- Ledger contains exactly the 49 distinct failing test names
- Each name has a log excerpt
- Each name carries an environment-bound or possible-regression tag with evidence

## Test plan

1. Compare deduped captured name count to 49
2. Spot-check a sample of tags against their cited log excerpts

## Verification commands and expected evidence

1. Confirm clean base 557240e checkout
2. cargo +1.98.1 test --locked --all-targets -- --test-threads=1 expecting exit 101 with 49 failures
3. Sort and count distinct failed names equals 49
4. Review ledger coverage in the report file

## Edge cases and failure handling

- Names spanning multiple binaries collapse correctly under dedupe
- Tests failing only intermittently still get captured in the single-threaded run

## Constraints

- Pinned toolchain: cargo +1.98.1 with --locked and single-threaded test runs, matching the F9 gate scripts
- CLR-009 repair-first ordering: the verified F9 edit stays held in its worktree and commits only after the base gate is green; the gate exception was declined
- No weakening of assertions; prefer test-harness changes over production-behavior changes, itemizing any unavoidable behavior change with justification (R3, R6)
- Standing rule: fmt, test, and clippy must all pass before any resulting commit (product:decisions)

## Out of scope

- The F9 index relabel itself (held work under F9)
- Relaxing the mandated fmt/test/clippy gate rule (separate operator decision)
- Repository renaming; the public-launch decision keeps `zbarno/kool.ade`
- Any product-behavior redesign beyond test plumbing

## Definition of done

- Ledger saved covering all 49 names with excerpts and classification evidence

## Instructions for the implementing model

Read this story and its linked dependencies before editing. Confirm the named existing files and interfaces; proposed files are explicitly labeled. Implement only this task's scope, preserving the constraints above. Run the verification commands and record actual results, changed files, and any remaining limitations. Do not mark complete on a build alone when acceptance criteria require runtime interaction. If an assumption contradicts the repository or approved specification, report the discrepancy rather than silently inventing a requirement.
