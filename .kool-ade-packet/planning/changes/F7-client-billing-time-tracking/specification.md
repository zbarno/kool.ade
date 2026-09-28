# F7: Client Billing Time Tracking
<!-- packet-artifact-id:v1 {"uid":"c72bff0b-3d5a-45e8-9833-27f0d0b8ebc8","displayId":"F7","title":"Client Billing Time Tracking"} -->
<!-- packet-change:v1 {"schemaVersion":1,"uid":"c72bff0b-3d5a-45e8-9833-27f0d0b8ebc8","displayId":"F7","status":"draft"} -->

**Status:** Draft — capture mechanism and attribution axis settled by operator rulings: automatic agent-activity capture (CLR-029) and workspace-only attachment (CLR-030). The report/export shape (CLR-031) remains an open board decision; those parts stay model-agnostic. Ready confers no implementation authority.

## Intent

Packet does not know how long the operator worked, or for whom. Billable hours live in memory or a reconstructed spreadsheet, invoices ship late or miss, and the operator keeps time bookkeeping outside Packet entirely. This feature makes time spent on client work reliably tracked inside Packet: attribute every counted minute to the workspace where it accrued, accumulate per-workspace hours, and produce per-workspace, per-period breakdowns the operator rolls up into client invoices in their own invoicing flow (workspace-to-client mapping is external, per the CLR-030 ruling). The operator's ruling fixes the honesty bar: time accrues only while work is actually progressing, never while Packet sits waiting on the operator.

## Current Behavior

Grounded in the authoritative product modules:

- No client or counterparty concept exists anywhere: module 07 (stores and core types), module 02 (scope), and module 03 (actors) define no client, rate, or invoice entity, and no biler role among actors; the CLR-030 ruling keeps it that way for v1.
- No time measurement: the feature inventory (F-1 through F-26) contains no time-tracking capability. The nearest neighbour, the card activity graph (F-19), plots observed activity updates in ten-second buckets — a liveness indicator, not labour hours, and it attributes nothing to anybody.
- Shaping constraints already in force: durable facts are git-backed project artifacts (NFR-1) applied atomically (NFR-2, with the one-checkpoint-per-turn culture of FR-3); network-neutrality posture (NFR-5); Linux x86_64 desktop from source (NFR-7).

## Desired Behavior

Accepted shape once the board decisions land:

- **Attribution.** Every counted minute resolves to exactly one workspace (the connected repository); work performed with no workspace context is never billable. Workspace-to-client mapping lives outside Packet (CLR-030); no client entity, roster, or label enters the app.
- **Capture (settled, CLR-029).** Time enters the ledger automatically, driven by agent activity: while an agent is actively executing on work attached to a workspace, that workspace's meter runs. The meter stops — and nothing accrues — whenever an agent is waiting on the operator (an unanswered question, a held approval, a pause), the queue is idle, a session is dormant, or the app is closed. Counted minutes are explicit intervals. There is no manual start/stop timer and no retrospective time-entry in the v1 capture path.
- **Truthtelling.** A workspace total is always the sum of its recorded intervals; waiting, unattended, unattributed, cancelled, or crashed time never inflates it. Standing invariant: exported total equals on-screen total equals the sum of counted intervals.
- **Reporting.** Per-workspace day and week totals are visible on the board/detail surfaces the operator already uses, and a finished period hands off in the chosen report form (shape open, CLR-031).

Tracking is an additive observer over the existing workflow: it joins no planning turn, queue advancement, or checkpoint.

## Scope

**In:** workspace-only attribution (the connected repository is the unit of account; the workspace-to-client mapping is external per the CLR-030 ruling); agent-activity-driven automatic capture (per CLR-029 ruling); a persistent git-backed time ledger; per-workspace day/week totals on existing board surfaces; a period export in the chosen form; isolation keeping ledger writes out of planning and execution.

**Out:** rates, tax, invoice emission, and payments (the export hands off to the operator's invoicing tool); in-app client administration — rosters, labels, renames, merges, workspace-to-client mapping (clients roll up externally, and a multi-workspace client bill composes outside Packet); multi-operator billing and client portals; cloud sync or remote capture; client inference from repository metadata; manual start/stop timers and retrospective time entries as the primary capture path (possible optional layers later at most); Windows and macOS builds.

## Affected Product Areas

- Module 02 (Scope): billable-time capability added; multi-operator attribution and invoice computation join the deferred list.
- Module 03 (Actors): the seated operator doubles as biler; no new seat classes in v1.
- Module 04 (Inventory): a new capability row at reconciliation.
- Module 05 (FRs): new requirements for agent-driven capture, attribution, ledger persistence, reporting, and export.
- Module 07 (Data Model): a time-ledger record as a dedicated artifact slot, keyed by the stable workspace/repository identity; no client entity (CLR-030).
- Module 08 (Architecture): a new core module owns the ledger and observes agent execution state to drive accrual start/stop; UI hooks the existing board/detail surfaces; the write path honours NFR-2 without flooding checkpoint history.

## Requirements

- **R1 (Attribution).** Counted time MUST resolve to exactly one workspace (the connected repository); work with no workspace context MUST NOT be billable. Packet MUST NOT maintain a client roster or workspace-to-client mappings (external per CLR-030).
- **R2 (Ledger).** Time records MUST be git-backed, atomic, and restore identically across restarts (NFR-1/NFR-2); the ledger is append-friendly so historical periods stay re-derivable.
- **R3 (Fidelity, settled by CLR-029).** A workspace total MUST equal the union of its active intervals: overlapping active time on one workspace counts once, and active time for different workspaces MAY accrue concurrently. Time while an agent waits on the operator, while queues sit idle, while sessions are paused or dormant, and unattributed, cancelled, and crash time MUST NOT contribute.
- **R4 (Visibility).** Per-workspace day and week totals MUST be visible on the existing board surfaces, including for the day in progress.
- **R5 (Export).** A completed period MUST export in the chosen report form (shape per CLR-031) with totals exactly equal to the displayed per-workspace totals.
- **R6 (Isolation).** Ledger writes MUST NOT disturb planning turns, queue progression, or board rendering, and MUST NOT initiate network activity (NFR-5).

## Decisions and Assumptions

- **AD-1 (assumed, reversible).** Solo-operator v1: one seat bills its own time; multi-operator attribution stays out of scope (matches the D-14 lightweight identity posture).
- **AD-2 (assumed, reversible).** Device-local clocks; timezones are a display-time concern with no DST correction; anomalous clocks are corrected by the operator.
- **AD-3 (agent-settled, CLR-032, reversible).** The ledger is a dedicated git-backed artifact slot in the project store, consistent with module 07's layout (which reserves `.kool-ade-packet/planning/` for shared planning truth) and NFR-1's “shared truth MUST be git-backed.” Commit cadence is coarse, not event-granular: FR-3 scopes git checkpoints to mutating *planning turns* (exactly one per accepted mutating turn, none on no-op), and per-minute commits would flood history contrary to the FR-3 culture and NFR-9 short-imperative-checkpoint phrasing. Concretely: ledger rows are written to the ledger file immediately and atomically (unique sibling temp, replace, dirsync — NFR-2), so restart durability rests on the file bytes plus git commits taken at coarse grains; git commits occur only at grains the ledger's owner chooses at task generation, expected candidates: session save/close, day rollover, period export, or explicit flush. Event-granular commits are ruled out; the precise trigger set is a task-generation detail, kept model-agnostic alongside the open export-form decision. Crash semantics for rows not yet covered by a commit are bounded by AC2 (restored or consistently discarded, never double-counted) and remain acceptable for a billing ledger whose primary audit trail is the committed history.
- **AD-4 (operator ruling, CLR-029).** Capture is automatic agent-activity tracking: meters run exactly while an agent is actively executing on work attributable to a client and record nothing while the system waits on the operator (unanswered question, held approval), sits idle, is paused, or is dormant. Explicit start/stop timers and retrospective log entries are not adopted as the primary mechanism; they remain possible optional layers for later. This ruling closes the three-way capture fork.
- **AD-5 (assumed, agent-settled, reversible).** Concurrency semantics: a client total is the union of its active intervals (overlaps on one client count once — the anti-inflation behaviour the ruling points at), while two agents advancing different clients concurrently accrue independently, so cross-client totals may exceed wall rate. Intervals carry the identity of the worker/session that produced them, so either policy stays re-derivable from the stored records if the operator later prefers a cap. Under the CLR-030 workspace attribution axis, read “client” as workspace throughout: overlaps on one workspace count once, different workspaces accrue independently.
- **AD-6 (operator ruling, CLR-030).** Attachment is workspace-only: the connected repository's workspace is the single attribution axis, and no client entity, roster, label, or mapping exists in the data model or the UI. The operator maps workspaces to clients in their own invoicing tool; totals, reports, and exports are per-workspace per period, and a client bill spanning multiple workspaces composes outside Packet (accepted limitation of this ruling). Ledger rows keep a stable attribution key field (the workspace/repository identity), so the history stays usable if client attachment is ever adopted later — though backfilling clients would still require external mapping judgement.
- **Open on the board (Human).** The report/export shape (CLR-031) is unsettled; R5/AC3 and the module-07 export shape follow that ruling. Everything above is deliberately agnostic to it.

## Acceptance Criteria

- **AC1 (attribution and truthtelling).** Minutes accruing while an agent actively works inside workspace W appear in W's day total within at most one minute of rounding; minutes accruing while no workspace-attached agent is active appear in no workspace total.
- **AC2 (persistence).** Kill and restart Packet mid-interval: all completed records restore byte-identically, and the interrupted interval is either counted or discarded per R3 consistently — never double-counted.
- **AC3 (reporting fidelity).** A mixed week across at least two workspaces produces day and week totals exactly matching an independent re-sum of the recorded intervals, and the export emits those same totals in the chosen report form.
- **AC4 (isolation).** With the tracker running continuously, planning turns complete, queue work advances, and the board renders with no stall or deadlock exercised by the regression suite.
- **AC5 (quality bar).** Full suite green; no new clippy warnings versus the recorded baseline at the pinned toolchain (NFR-8/D-34).
- **AC6 (wait-exclusion and union).** Holding an agent in a state that awaits the operator (unanswered question or held approval) for several minutes accredits zero minutes to that workspace; after release, accrual resumes cleanly with no backfill of the held window. With two workers running concurrently on the same workspace, that workspace's day total grows at no faster than one times wall rate.
