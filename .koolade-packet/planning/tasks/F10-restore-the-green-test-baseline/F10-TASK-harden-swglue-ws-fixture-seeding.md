---
<!-- koolade-artifact-id:v1 {"uid":"4b761a09-4fa9-41f9-b006-4882ccf86e64","displayId":"F10-TASK-harden-swglue-ws-fixture-seeding","title":"Harden swglue ws fixture seeding","parentUid":"106cdc43-203a-42fb-8104-297c38b9b8c0"} -->

koolade-task: {"schemaVersion":1,"uid":"4b761a09-4fa9-41f9-b006-4882ccf86e64","batchUid":"106cdc43-203a-42fb-8104-297c38b9b8c0","repositoryId":"root","dependencyUids":["f27bc9f0-f995-4ad6-81fc-aeeac7f1f562"]}
---

# F10-TASK-harden-swglue-ws-fixture-seeding — Harden swglue ws fixture seeding

Feature: Restore the green test baseline (F10)

Status: Individually validated; see batch index for generation status.

## Problem this ticket solves and why

Some of the 49 base test failures were attributed to unstable fixture seeding in the worker sandbox, but no swglue or WebSocket symbol exists in the visible source. Story 1's ledger will bind the term to concrete test names; until then the actual non-determinism sites are unknown and these tests keep the gate red.

## Ticket goal — what changes when done

Every swglue-family test from the ledger passes deterministically under the pinned single-threaded run, with production code untouched and assertions unweakened.

## User story

As the developer-operator I want the seeded fixtures reproducible so repeated runs yield identical results and stop generating spurious red signals.

## Purpose

Stabilize the environment-bound swglue WebSocket fixture family flagged by the worker report via deterministic seeding and isolated scratch state, without touching production behavior or assertion strength.

## Specification references

Source: [Approved specification](specification.md)

## Implementation context

Story 1 supplies the 49-name ledger. The F9 report calls out a swglue/ws fixture-seeding family as environment-bound, but no matching symbol or WebSocket dep appears in Cargo.toml or src/, so module paths and the specific randomness sources are undetermined and must be resolved from the ledger plus targeted source reads. Only names tagged environment-bound belong here; possible-regression names go to story 5.

Feature ID: F10
Repository: root

## Approved scope mapping

- Scope 3: Remediate every environment-bound failure by hardening test scaffolding: unique isolated scratch locations, deterministic seeding, reliable cleanup (R3)
- Success criterion 1: `cargo test --locked --all-targets -- --test-threads=1` at base 557240e exits 0 with 0 failed, with a log excerpt recorded (AC1)
- Success criterion 3: fmt and clippy (-D warnings, --all-targets) are green; the remediation diff is test-harness-heavy and any unavoidable behavior change is itemized per R6 (AC3)

## Dependencies

- [F10-TASK-capture-and-classify-the-49-failure-baseline](F10-TASK-capture-and-classify-the-49-failure-baseline.md) must be complete.

## Affected files and components

- Swglue-family test modules/helpers located via story 1's ledger (paths TBD; likely #[cfg(test)] blocks under src/ submodules)
- Shared scratch or seed utilities if an existing helper serves multiple tests

## Implementation steps

1. Extract the environment-bound swglue-family subset from the ledger; map each name to a concrete test fn with cargo test --list.
2. Find the non-deterministic source per test (random UUID, wall clock, shared /tmp path, unordered iteration, stale inter-run fixture).
3. Replace with deterministic equivalents: fixed seed or counter, pinned chrono::DateTime, per-test unique scratch dir under env::temp_dir(), removal of leaking global state.
4. Add cleanup so repeated runs create no residue.
5. Change no production code and relax no assertions; itemize any forced production touch per R6.
6. Log per-test seeds, paths, and untouched assertion lines in the report area.

## Acceptance criteria

- Family subset passes on two consecutive pinned gate runs with no newly failing tests elsewhere.
- clippy -D warnings and fmt --check introduce no new violations.
- Diff touches only #[cfg(test)], test helpers, or fixture data; no assertion comparison changed.
- Per-test before/after evidence appended to the report.

## Test plan

1. Back-to-back full-suite runs: family names flip fail→pass, no new failures.
2. --exact repeat ×5 on the smallest family member: all pass, no leftover scratch dirs afterwards.
3. diff of test bodies vs base 557240e confirms operands/thresholds untouched.

## Verification commands and expected evidence

1. cargo +1.98.1 test --locked --all-targets -- --test-threads=1 (family cleared; suite may stay red on other families)
2. cargo +1.98.1 clippy --locked --all-targets -- -D warnings (exit 0)
3. cargo +1.98.1 fmt --all --check (exit 0)

## Edge cases and failure handling

- Ledger mis-tags a family-looking test as possible-regression: defer it to story 5.
- Cleaner removes checked-in fixture asset because scratch dir nests beside it—restrict deletion to runtime-created paths only.

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

- Family names absent from failed set across two runs; per-test remediation noted in report.

## Instructions for the implementing model

Read this story and its linked dependencies before editing. Confirm the named existing files and interfaces; proposed files are explicitly labeled. Implement only this task's scope, preserving the constraints above. Run the verification commands and record actual results, changed files, and any remaining limitations. Do not mark complete on a build alone when acceptance criteria require runtime interaction. If an assumption contradicts the repository or approved specification, report the discrepancy rather than silently inventing a requirement.
