# F7: Client Billing Time Tracking
<!-- packet-artifact-id:v1 {"uid":"c72bff0b-3d5a-45e8-9833-27f0d0b8ebc8","displayId":"F7","title":"Client Billing Time Tracking"} -->
<!-- packet-change:v1 {"schemaVersion":1,"uid":"c72bff0b-3d5a-45e8-9833-27f0d0b8ebc8","displayId":"F7","status":"draft"} -->

**Status:** Draft — intent captured. The capture mechanism, billing unit, and invoice-handoff shape are open board decisions; requirements and acceptance criteria stay model-agnostic until those settle. Ready confers no implementation authority.

## Intent

Packet does not know how long the operator worked, or for whom. Billable hours live in memory or a reconstructed spreadsheet, invoices ship late or miss, and the operator keeps time bookkeeping outside Packet entirely. This feature makes time spent on client work reliably tracked inside Packet: attribute work to a client, accumulate the hours, and produce a per-client, per-period breakdown the operator can bill from directly.

## Current Behavior

Grounded in the authoritative product modules:

- No client or counterparty concept exists anywhere: module 07 (stores and core types), module 02 (scope), and module 03 (actors) define no client, rate, or invoice entity, and no biler role among actors.
- No time measurement: the feature inventory (F-1 through F-26) contains no time-tracking capability. The nearest neighbour, the card activity graph (F-19), plots observed activity updates in ten-second buckets — a liveness indicator, not labour hours, and it attributes nothing to anybody.
- Shaping constraints already in force: durable facts are git-backed project artifacts (NFR-1) applied atomically (NFR-2, with the one-checkpoint-per-turn culture of FR-3); network-neutrality posture (NFR-5); Linux x86_64 desktop from source (NFR-7).

## Desired Behavior

Accepted shape once the board decisions land:

- **Attribution.** Every counted minute resolves to exactly one named client; unattributed work is never billable.
- **Capture.** Time enters the ledger by the operator-chosen mechanism (passive foreground counting with idle discard, explicit start/stop timers, or retrospective log entries). Whichever is picked, counted minutes are explicit intervals, not estimates.
- **Truthtelling.** A client total is always the sum of recorded intervals; unattended, idle, cancelled, or crashed time never inflates it. Standing invariant: exported total equals on-screen total equals the sum of counted intervals.
- **Reporting.** Per-client day and week totals are visible on the board/detail surfaces the operator already uses, and a finished period hands off in the chosen invoice form.

Tracking is an additive observer over the existing workflow: it joins no planning turn, queue advancement, or checkpoint.

## Scope

**In:** client identification; the capture mechanism (per board ruling); a persistent git-backed time ledger; per-client day/week totals on existing board surfaces; a period export in the chosen form; isolation keeping ledger writes out of planning and execution.

**Out:** rates, tax, invoice emission, and payments (the export hands off to the operator’s invoicing tool); multi-operator billing and client portals; cloud sync or remote capture; client inference from repository metadata; Windows and macOS builds.

## Affected Product Areas

- Module 02 (Scope): billable-time capability added; multi-operator attribution and invoice computation join the deferred list.
- Module 03 (Actors): the seated operator doubles as biler; no new seat classes in v1.
- Module 04 (Inventory): a new capability row at reconciliation.
- Module 05 (FRs): new requirements for capture, attribution, ledger persistence, reporting, and export.
- Module 07 (Data Model): client identity (shape per ruling) plus a time-ledger record as a dedicated artifact slot.
- Module 08 (Architecture): a new core module owns the ledger; UI hooks the existing board/detail surfaces; the write path honours NFR-2 without flooding checkpoint history.

## Requirements

- **R1 (Attribution).** Counted time MUST resolve to exactly one client; unrecorded work MUST NOT be billable.
- **R2 (Ledger).** Time records MUST be git-backed, atomic, and restore identically across restarts (NFR-1/NFR-2); the ledger is append-friendly so historical periods stay re-derivable.
- **R3 (Fidelity).** A client total MUST equal the sum of its counted intervals; idle, unattributed, cancelled, and crash time MUST NOT contribute. The mechanism follows the capture-model ruling.
- **R4 (Visibility).** Per-client day and week totals MUST be visible on the existing board surfaces, including for the day in progress.
- **R5 (Export).** A completed period MUST export in the chosen invoice form with totals exactly equal to the displayed totals.
- **R6 (Isolation).** Ledger writes MUST NOT disturb planning turns, queue progression, or board rendering, and MUST NOT initiate network activity (NFR-5).

## Decisions and Assumptions

- **AD-1 (assumed, reversible).** Solo-operator v1: one seat bills its own time; multi-operator attribution stays out of scope (matches the D-14 lightweight identity posture).
- **AD-2 (assumed, reversible).** Device-local clocks; timezones are a display-time concern with no DST correction; anomalous clocks are corrected by the operator.
- **AD-3 (assumed, agent-owned).** The ledger is a dedicated artifact in the project store; its commit cadence (event-granular versus coarse checkpoints) is decided at task generation to balance NFR-2 durability against history flooding.
- **Open on the board (Human).** Capture mechanism and billing unit are unsettled; the R3 mechanism and the module-07 shape follow those rulings. Requirements and ACs are deliberately model-agnostic until then.

## Acceptance Criteria

- **AC1 (attribution and truthtelling).** Minutes recorded against client A appear in A’s day total within at most one minute of rounding; minutes recorded against no client appear in no client total.
- **AC2 (persistence).** Kill and restart Packet mid-interval: all completed records restore byte-identically, and the interrupted interval is either counted or discarded per R3 consistently — never double-counted.
- **AC3 (reporting fidelity).** A mixed week across at least two clients produces day and week totals exactly matching an independent re-sum of the recorded intervals, and the export emits those same totals in the chosen invoice form.
- **AC4 (isolation).** With the tracker running continuously, planning turns complete, queue work advances, and the board renders with no stall or deadlock exercised by the regression suite.
- **AC5 (quality bar).** Full suite green; no new clippy warnings versus the recorded baseline at the pinned toolchain (NFR-8/D-34).
