# CHG-002: Board Card Display — Red-Line Activity Graph and Class-Colored Cards

**Status:** Ready
**Directed by:** Operator directives recorded as D-26 and D-27; F-20 typology locked by the operator's CLR-020 answer.
**Bound:** Additive UI features only. The binding §12 MVP exit bar is untouched (D-21).

## Intent

During the long quiet stretch of the ticket-007 exit demonstration, the board was the operator's only honest signal — and a flat, undifferentiated board forces proxied polling to tell "slow" from "wedged." This feature turns the kanban board into a situational display with two additive, operator-directed changes:

- **F-19 (D-26):** the task activity graph MOVES from the task details view onto the task card on the kanban board, redrawn as a RED LINE in place of the accent bars. The activity metric is preserved unchanged: observed update counts in ten-second buckets over a rolling 10-minute / 60-slot window.
- **F-20 (D-27):** board cards receive a color treatment dependent on the TYPE of card, so the mix of work on the board reads at a glance. Typology LOCKED by the operator's CLR-020 answer ("Yes card class"): the classes the board already prints — Task-story cards headed `{key} · Task` versus open-item cards headed `{id} · {kind}` with kind ∈ Question / Ambiguity / Assumption / Ownership.

Not owed: new telemetry, token accounting, animation, selectable palettes, and any core, harness, data-model, schema, prompt, or artifact change. Alternatives ruled out by the operator's confirmation: work-phase, code-area, and priority-tier coloring axes.

## Current Behavior

Code-grounded, read-only audit of master before CHG-002:

- `board_card` (`src/ui/layout.rs:656`) paints every kanban card identically: `theme::PANEL` fill (rgb 43,43,43), corner radius 6, 1 px `theme::BORDER` stroke (65,65,65). The only variance is the ACTIVE (worker-running) card, which swaps the stroke to `theme::ACCENT` (218,223,212). Card bodies print headers `TASK-### · Task` or `CLR-### · {kind}` in ACCENT text.
- Card activity is text-only: `task_activity::compact` (`src/ui/task_activity.rs`) renders a LIVE/LAST ACTIVITY tag, a ≤200-character preview of the newest post/response/thought/activity string, an `M SS · N updates` timing line, and a "View all activity" button. There is no chart on the card.
- The bar graph exists only in `task_activity::full` (task details modal and the View-all-activity path, invoked at `src/ui/layout.rs:506`): accent-filled rectangles per nonzero bucket in a 60-slot × 54 px area; bucket index = `epoch_ms / 10_000 − 59 + i`; heights normalized by `max(sample_peak, 1)`; hover copy explains that empty space does not mean the worker stopped; caption "Activity updates / 10s · last 10 minutes · token usage is not reported"; below it, the full scrollable post stream via `chat_pane::paint_progress`.
- The telemetry feeding both renderings is `LiveProgress.telemetry.samples: Vec<(i64, u32)>` — update counts per ten-second bucket — serialized with the progress record, surviving relaunch (unit-tested in `task_activity.rs`).
- The palette is centralized in `src/ui/theme.rs`. The tested `ItemKind::badge_colors` already encodes the kinds: Question → PURPLE (167,139,250), Ambiguity → SUCCESS (107,212,144), Assumption → lavender (190,168,255), Ownership → pink (232,145,190). `theme::DANGER` red (255,107,107) is used for blocking emphasis today and is not a card hue.
- Mixed board columns interleave item and task cards; the card class is structurally known at paint time (two distinct body closures in the `layout.rs` board body), so no schema, data-model, or story-format change is owed.

## Desired Behavior

**F-19 — red line graph on the task card.**

- Each task-story card on the board renders a compact red POLYLINE over the same sampled activity series: sixty ten-second buckets, a ten-minute window ending at the current minute for an active card and the record's final-updated minute for a settled card, y-values = updates per bucket scaled to `max(peaks, 1)`.
- The line is stroked in `theme::DANGER` red at roughly 1.5 px with no bar fills; a faint baseline renders so a zero-activity window reads as a flat line rather than void.
- The metric-meaningful explanation survives: on hover or in caption — updates per 10-second bucket, last 10 minutes, token usage is not reported, and empty buckets do not mean the worker stopped.
- An active card's window rolls with the clock (right edge advances between repaints); a settled card shows its final recorded window statically.
- Degenerate telemetry — no samples at all, all-zero buckets, a window entirely before any recorded sample — renders the flat baseline without errors or invented spikes.

**F-19 — details view keeps its stream.**

- The task details / View-all-activity view RETAINS the full post stream (sticky-bottom scroll area) and the timing header, but the duplicated accent-bar subchart is MOVED, not mirrored: the card's red line is the single chart representation of the metric for a task (assumption P1 — overridable by a fresh word without reopening D-26).

**F-20 — class-tinted card frames.**

- Every kanban card frame wears a hue keyed to its printed work class, from a centralized mapping in `src/ui/theme.rs`. Recorded defaults (planner-chosen because the operator expressed no hue preference; overridable by a fresh word):

| Class | Card hue |
| --- | --- |
| Task-story card | `theme::ACCENT` sage (218,223,212) — house hue |
| Question | PURPLE (167,139,250) |
| Ambiguity | SUCCESS green (107,212,144) |
| Assumption | lavender (190,168,255) |
| Ownership | pink (232,145,190) |

- Rendering is a toned-down fill and/or class-hue stroke on the existing `board_card` frame, tuned for contrast against the dark palette; body text colors are unchanged. Item cards thus echo the hues their kind badges already wear, keeping one color vocabulary across panels and board.
- `theme::DANGER` red is DELIBERATELY LEFT UNCLAIMED as a card hue so the F-19 red activity line stays singular on the very cards it decorates.

**F-20 — active state stays legible.**

- A worker-running card MUST remain visually distinguishable from an idle card of the SAME class. Because the Task base hue already equals ACCENT (today's active-marker color), the running Task card earns a heavier treatment — wider/heavier stroke and/or an `ACCENT_SOFT`-toned fill shift. Other classes keep the ACCENT-stroke upgrade, which contrasts structurally against their bases.
- Class identity never relies on color alone: the printed `{key} · Task` / `{id} · {kind}` headers remain on every card.
- Contingency from the ruling: Question and Assumption share the purple family; if they collapse as card frames at a glance, the FIRST hue swap to make is one of those two frame hues (candidate: Assumption toward a warmer distinguishable tone) by fresh word — without re-opening the typology ruling.

## Scope

In scope:

1. A small, reusable compact line-chart painter for a 60-slot bucket series (inputs: samples, window-anchor minute, line color, size; no new telemetry plumbing) used by the board's task cards.
2. `board_card` gaining a work-class parameter (or equivalent variant split) selecting fill/stroke; the two existing board-body branches pass Task versus the item's kind at their current paint sites.
3. Removing the bar sub-render from `task_activity::full` while preserving its header, timing line, and full stream; metric and explanatory copy retained on the card chart.
4. Centralized class→hue mapping plus active-state variants in `src/ui/theme.rs`; updated hover/caption strings.
5. Unit tests: bucket/window math and degenerate windows for the line painter; exhaustive class mapping; active-versus-idle stroke inequality; DANGER-red-not-a-card-hue guard.

Out of scope:

- Any new telemetry source, token-usage reporting, or sampling-semantics change.
- Charts or class tinting in other surfaces (items pane badges, settings, spec viewer).
- Animation, user-selectable palettes, or alternate/high-contrast themes.
- Anything outside `src/ui/`: no core, harness, artifacts, prompts, data model, or story-format changes.

## Affected Product Areas

- **Module 04 (Feature Inventory):** F-19 and F-20 flip Planned → Implemented at reconciliation; no other F-numbers touched.
- **Module 05 (Functional Requirements):** FR-19 (Kanban projection) is the nearest standing obligation; no new product MUST is admitted for unmerged behavior (FR-15 / D-28 discipline — the feature document carries the requirements until reconciliation).
- **Module 12 (Acceptance):** explicitly unchanged; its last paragraph already excludes F-19/F-20 from the binding bar.
- **Module 13 (Source Map):** the current-authority row and the desktop-board row gain this feature document.
- **Code:** `src/ui/theme.rs`, `src/ui/task_activity.rs`, `src/ui/layout.rs` plus their in-file unit tests. Telemetry shape (`LiveProgress.telemetry.samples`) is reused verbatim.

## Requirements

- **REQ-F19-1 (MUST).** A task-story card on the kanban board renders its activity as a `theme::DANGER`-red polyline over `LiveProgress.telemetry.samples` using the SAME bucket arithmetic as the outgoing bars: bucket index `floor(epoch_ms / 10_000)`, a 60-bucket ten-minute window ending at the current minute (active) or the record's last-updated minute (settled), peak normalization `max(samples, 1)`.
- **REQ-F19-2 (MUST).** The card chart carries the metric-meaningful explanation (updates per 10-second bucket · last 10 minutes · token usage is not reported · empty buckets do not mean the worker stopped) via hover or caption.
- **REQ-F19-3 (MUST).** The details / View-all-activity view retains the full post stream and timing; the accent-bar subchart does not survive the move, so at most one chart represents a task's activity at a time.
- **REQ-F19-4 (MUST).** Degenerate telemetry (empty samples, all-zero buckets, window preceding all samples) renders a flat baseline with no crash and no fabricated spikes.
- **REQ-F20-1 (MUST).** Every kanban card's frame hue derives from its printed work class through the centralized mapping; the mapping excludes `theme::DANGER` red from all card hues.
- **REQ-F20-2 (MUST).** A worker-running card stays visually distinguishable from an idle card of the same class, INCLUDING Task cards whose base hue equals ACCENT.
- **REQ-F20-3 (MUST).** Printed class headers remain on every card; color alone never signals class.
- **REQ-ALL-1 (MUST).** The merged commit builds warning-free (NFR-8), passes the full regression suite (203 incumbent test functions plus the new tests), and confines its diff to `src/ui/` and in-file tests.

## Decisions and Assumptions

- **D-26 (confirmed operator directive):** the activity graph moves to task cards, redrawn as a red line, metric preserved.
- **D-27 (confirmed operator directive):** card color depends on the type of card, type-based palette.
- **D-29 gate clearance:** the operator's "yes" to the generation offer is the explicit implementation approval D-29 owes this Ready document. That approval becomes operative only when the application registers it in its approval ledger (`.planner/workflow.json` approved_features) through the in-app **Approve feature for implementation** action on the active feature — that ledger entry, not this prose, releases the generation gate, and the ledger snapshot must equal this document (a later edit to the document lapses the approval until re-approval). The approval grants nothing downstream on its own: the D-29 order stands, P2 sequencing unchanged — ticket 007 resumes from its board card first, then Implement & continue walks the queue.
- **CLR-020 (resolved, archived v1.4):** typology locked to the class the board already prints ("Yes card class") — Task-story versus open-item by kind; phase/code-area/priority axes ruled out by the operator's confirmation. No schema or data-model work owed; the branch is known structurally at paint time.
- **Palette defaults (recorded defaults, overridable by a fresh word):** per-class hues per the table above; DANGER red unclaimed as a card hue (stipulation ii); active-state differentiation guaranteed incl. Task (stipulation i); Question/Assumption purple-family collapse handled by swapping one frame hue first (stipulation iii).
- **P1 — assumption (agent grade, reversible):** "move" means the details view drops its accent-bar subchart while KEEPING the full text stream. Correctable by a word at story review; does not reopen D-26.
- **P2 — sequencing:** the two stories slot BEHIND ticket 007 (the binding seven-outcome demonstration, currently Needs attention with preserved work, resumable from its board card). F-19 and F-20 are functionally independent but touch neighboring files; implement F-19 first (smaller blast radius in `task_activity.rs`) to keep merges clean.
- **P3 — geometry:** the compact chart occupies a bounded band (~40–56 px tall, card-width) under the card's existing activity preview; cards grow slightly in height, absorbed by the existing column scroll. Exact pixels are the implementing story's discretion within the band.

## Acceptance Criteria

Each observable in the built app against the seeded fixture repository:

1. A task card with nonzero telemetry shows a red polyline whose tallest segments sit at exactly the buckets holding the most updates (verifiable against the serialized `LiveProgress` samples); the line is DANGER red; no bar fills render on the card.
2. Hovering the card's chart reveals the metric-meaningful copy (updates/10 s, last 10 minutes, empty ≠ stopped, token usage not reported).
3. A settled task's card shows the static final 10-minute window of its recorded samples; an active task's card advances its window's right edge between repaints.
4. A card whose telemetry is empty or all-zero renders a flat baseline: no panic, no placeholder spikes, caption intact.
5. Opening the task details / View-all-activity view shows the full post stream and timing; the accent bar graph is gone from it.
6. On a mixed fixture board: task cards read sage, Question cards purple, Ambiguity green, Assumption lavender, Ownership pink; a running task card is distinguishable from an idle task card (stroke weight or fill); every card still prints its class header.
7. `cargo clippy --all-targets --offline` reports no warnings and `cargo test --offline` passes — the incumbent 203-test suite plus the new painter and mapping tests.
8. The merged diff touches only `src/ui/theme.rs`, `src/ui/task_activity.rs`, and `src/ui/layout.rs` (plus in-file tests); reconciliation then flips F-19/F-20 to Implemented in module 04, extends module 13, and leaves the §12 bar untouched.
