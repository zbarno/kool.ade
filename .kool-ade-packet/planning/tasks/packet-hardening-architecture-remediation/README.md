# Packet Hardening and Architecture Remediation

**Status:** In progress  
**Current phase:** Phase 2 — complete the `.kool-ade-packet` migration
**Last checked:** 2026-09-25  
**Source plan:** [Full user-provided plan](source-plan.md)  
**Initial code baseline:** `eb001368949d`

This is the durable work ledger for the supplied 27-phase remediation plan. The source plan preserves the complete requirements and acceptance criteria. A phase is marked complete only after its own acceptance criteria have current evidence; existing code or a passing build alone does not count.

Phase 0 is recorded in [remediation-baseline.md](../../archive/remediation-baseline.md). Its required baseline checks and artifact inventory are complete. Each later phase must be updated here as implementation and evidence change. Keep commits focused and in the source plan's dependency order.

| ID | Task | Status |
| --- | --- | --- |
| P0 | [Establish green baseline](source-plan.md#4-phase-0-establish-a-green-baseline) | Complete |
| P1 | [Introduce a single artifact layout authority](source-plan.md#5-phase-1-introduce-a-single-artifact-layout-authority) | Complete |
| P2 | [Complete the `.kool-ade-packet` migration](source-plan.md#6-phase-2-complete-the-kool-ade-packet-migration) | Not assessed |
| P3 | [Fix Git index isolation](source-plan.md#7-phase-3-fix-git-index-isolation) | Not assessed |
| P4 | [Replace stringly typed workflow state](source-plan.md#8-phase-4-replace-stringly-typed-workflow-state) | Not assessed |
| P5 | [Introduce stable internal identities](source-plan.md#9-phase-5-introduce-stable-internal-identities) | Not assessed |
| P6 | [Stop recovering machine state from Markdown](source-plan.md#10-phase-6-stop-recovering-machine-state-from-markdown) | Not assessed |
| P7 | [Redesign the planner decision model](source-plan.md#11-phase-7-redesign-the-planner-decision-model) | Not assessed |
| P8 | [Replace brittle conversational phrase matching](source-plan.md#12-phase-8-replace-brittle-conversational-phrase-matching) | Not assessed |
| P9 | [Add generative bounded retrieval planning](source-plan.md#13-phase-9-replace-fixed-context-heuristics-with-generative-retrieval-planning) | Not assessed |
| P10 | [Make specification structure adaptive](source-plan.md#14-phase-10-make-specification-structure-adaptive) | Not assessed |
| P11 | [Remove arbitrary task word counts](source-plan.md#15-phase-11-remove-arbitrary-task-word-counts) | Not assessed |
| P12 | [Redesign ADR generation](source-plan.md#16-phase-12-redesign-adr-generation) | Not assessed |
| P13 | [Harden LLM execution boundaries](source-plan.md#17-phase-13-harden-llm-execution-boundaries) | Not assessed |
| P14 | [Split automatic planning, building, and publication](source-plan.md#18-phase-14-split-automatic-planning-building-and-publication) | Not assessed |
| P15 | [Add independent CI and quality gates](source-plan.md#19-phase-15-add-independent-ci-and-quality-gates) | Not assessed |
| P16 | [Decompose core god modules](source-plan.md#20-phase-16-decompose-core-god-modules) | Not assessed |
| P17 | [Replace the giant UI Surface interface](source-plan.md#21-phase-17-replace-the-giant-ui-surface-interface) | Not assessed |
| P18 | [Normalize harness capabilities](source-plan.md#22-phase-18-normalize-harness-capabilities) | Not assessed |
| P19 | [Split response schemas by operation](source-plan.md#23-phase-19-split-response-schemas-by-operation) | Not assessed |
| P20 | [Consolidate planner policy](source-plan.md#24-phase-20-consolidate-planner-policy) | Not assessed |
| P21 | [Improve nontechnical decision UX](source-plan.md#25-phase-21-improve-nontechnical-decision-ux) | Not assessed |
| P22 | [Harden persistence and atomic writes](source-plan.md#26-phase-22-harden-persistence-and-atomic-writes) | Not assessed |
| P23 | [Improve logging and secret hygiene](source-plan.md#27-phase-23-logging-and-secret-hygiene) | Not assessed |
| P24 | [Complete the final test matrix](source-plan.md#30-final-test-matrix) | Not assessed |
| P25 | [Remove superseded compatibility code](source-plan.md#32-implementation-order) | Not assessed |
| P26 | [Update current product truth and ADRs](source-plan.md#32-implementation-order) | Not assessed |

## Continuation notes

- Follow the implementation order in section 32 of the source plan; later phases depend on earlier migrations.
- Update this table and add concrete evidence whenever a phase changes state.
- Treat all source-plan acceptance criteria and the definition of done as binding. Do not mark the initiative complete while any final matrix item is unverified.
- The current repository still uses legacy root `planning/`, `.planner/`, `adr/`, and `SPECIFICATION.md` paths alongside `.kool-ade-packet/`; this is an observed starting condition, not a completed migration.

## Phase evidence

- **P1 — artifact layout authority:** `src/artifacts/layout.rs` now owns canonical and legacy project paths and validates dynamic path components. Runtime artifact paths in the named modules route through it; remaining direct examples are test fixtures or explanatory text. `cargo test --offline --all-targets --quiet -- --test-threads=1` passed 487 tests (one one-shot migration test intentionally ignored); layout containment tests and targeted formatting checks pass. Existing legacy path selection remains active for the upcoming migration phase.
- **Issue-specific blocker explanations:** production explanations are generated from each task's saved report and referenced documents. The generator now also receives recorded acceptance evidence, and explicit option IDs are discovered from the report without an `a`–`h` ceiling. Focused tests and the full serial suite pass. The sample quota explanation remains test-only fixture data.
