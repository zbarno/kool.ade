# Prepare this computer: the autonomous setup flow

Issue [#554](https://github.com/zbarno/kool.ade/issues/554) turns the scattered
machine-setup "Needs attention" queue into one coherent, Manager-led
experience. The user sees a single checklist — **Prepare this computer** —
and a single action: **Fix it for me**. Everything else stays the same:
individual scans, individual detail views, and the existing scoped
remediation brokers that perform and verify each change.

## What the user sees

- **One parent card** in the attention lane ("Prepare this computer") grouping
  every machine check. Each row shows a state marker (ready, working, waiting
  for your approval, waiting on another step, problem) and a per-row **Fix**
  affordance. Rows open the same per-step detail views as before.
- **Overall progress**: "N of M checks ready".
- **One sentence telling you where things stand** (working, waiting on X,
  paused, stopped, complete).
- **Controls**: *Fix it for me* (start/resume/restart), *Pause*, *Stop*, and
  *Check again* (rescan).
- **Plain-English consent panels** on whichever step the flow is waiting on:
  what will change, why, the security impact, and — expandable — whether it is
  reversible and what declining costs. Buttons: **Allow this change** or
  **Decline**.

All copy is static, curated, or derived from scanned task text. Models are
never allowed to invent consent copy, and the app never asks for passwords,
keys, tokens, or OTP codes.

## How it works

### Persisted state

`SetupFlowStateV1` persists beside the machine-scoped setup task store
(schema bumped 2→3 additively):

- `phase` — `Running`, `WaitingForConsent{task_id}`,
  `WaitingForUser{task_id, reason}`, `Finishing`, `Paused`,
  `Complete(outcome)`, or `Halted{reason, detail_task}`.
- `started` / `skipped` — operations the operator declined; they are not
  re-proposed for the life of the run and disappear only when the operator
  explicitly starts a fresh flow.
- `last_ready` / `last_completed` snapshots power honest readiness-regression
  detection (a check that *was* verifying must not silently vanish).
- Records are capped (≤32 completed / ≤16 seen / ≤16 halted) and the store
  enforces the caps at the persistence boundary, so no client can grow the
  file unboundedly.

### Decision driver (pure, clock-free)

[`crate::app::setup_workflow`] receives a `BeatInput` (tasks, controller
slots, scan/probe state, timestamps) and emits exactly one
`FlowAction` per beat: propose the highest-priority fix, hold for consent,
request a rescan (respecting a 15s anti-flap cooldown), transition, finish
(`FullyPrepared` or `PreparedWithUnavailable`), or halt with a reason
(readiness regression, repeated failures, interrupted-by-restart, …).
No randomness, no clocks, no network — deterministic and exhaustively
unit-tested. The controller families (remediation, locked-image
preparation, Claude install) are **serial executors**: the flow proposes
only when a family's slot is free, so two operations never contend inside
one broker.

### App responsibilities

`KooladeApp::pump_setup_flow` beats the driver every ~1.5s while connected:
reads the durable flow from disk, observes verified completions, tracks scan
generations, consults the three controller slots, and applies the emitted
action (routing a proposed fix straight into the existing
`request_setup_task_fix` pipeline). A persisted `Running` flow becomes
`Halted(InterruptedByRestart)` at boot — nothing resumes invisibly.

Commands: **StartSetupFlow** (new or resume; restarting from a finished or
stopped flow begins a clean slate without dropping already-verified steps),
**PauseSetupFlow**, **StopSetupFlow**, **DeclineSetupOperation** (records the
decline, withdraws any staged preview, and continues the flow).

### Security boundary (unchanged, reinforced)

The flow only **decides which existing, route-dispatched operation to
attempt and when**. It introduces no new operations, no auto-approved
mutations, and no privilege escalation. Every write to the machine still
passes the per-operation scoped consent gate in the remediation/image
brokers; the "Allow this change" button simply re-enters that same flow.
Declines are honored durably. See [machine-bootstrap.md](machine-bootstrap.md#safety-contract)
for the full safety contract.

## Tests

- Pure driver: ordering, slot gating (family-serial), consent holds,
  rescan cooldowns, completion and every halt reason (see
  [`driver/tests.rs`](../src/app/setup_workflow/driver/tests.rs)).
- Persistence: flow transactions, caps, legacy normalization
  ([`setup_tasks/transaction/tests.rs`](../src/persistence/setup_tasks/transaction/tests.rs)).
- Presentation: row markers/summaries/eligibility for the parent card
  ([`machine_group_tests.rs`](../src/ui/layout/board/columns/cards/machine_group_tests.rs)).
