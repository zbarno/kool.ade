# UI readability and polish

Status: complete

## Scope

Review every screen after the UI overhaul: welcome, board, specification and
conversation, new task, implementation and planning details, settings, workspace
menus, and auxiliary dialogs. Restore blue activity charts on task cards.
Preserve workflow behavior and use measured telemetry only.

## Work and evidence

- Header: replace overlapping foreground title with a responsive document layout;
  expose Settings directly and group workspace actions.
- Cards: measure content height to prevent overflow; restore activity chart,
  gradient fill and latest-update highlight; include the current live bucket.
- Settings: group Application, Work & Tools, and Project; use compact selector;
  move queue controls with automation and constrain desktop reading width.
- Details: replace cramped nested scrolling with a continuous narrow layout.
- Theme: improve metadata contrast and use coherent blue selection states.
- Screen review: production painters rendered through the eframe GPU renderer
  at 1600×900, 1280×720, 900×720, 360×720, 360×480 and 1280×480. All 28 scenes per
  size captured successfully with synthetic data. See
  [review and reproduction](../../../../docs/ui-polish-review.md).
- Independent review: completed with no blocking findings. Resolved settings
  layout direction, button contrast, card child-click handling, planning history
  charts, redundant scrolling, activity-footer sizing and short-window navigation.
- Focused interaction checks passed for measured card growth, child actions,
  activity windows and footers, branch selection, compact Settings and plan choice.
- Final pinned Rust gates (2026-10-07):
  - `cargo +1.98.1 fmt --all --check`: passed.
  - `cargo +1.98.1 test --locked --all-targets -- --test-threads=1`: passed;
    1,030 library and 4 integration tests, zero failures, 2 opt-in tests ignored.
  - `cargo +1.98.1 clippy --locked --all-targets -- -D warnings`: passed.
  - The ignored GPU gallery was run separately and passed (168 captures).
  - `git diff --check`: passed.

## Review boundary

The rendered review exercises production screen painters and pointer/scroll
events with synthetic state. It does not launch live coding workers or verify
external service availability. No workflow or persistence schema was changed.

## Completion requirements

- [x] All named screens reviewed in rendered output, including scroll/close behavior.
- [x] No overlapping text, unreachable primary actions, or clipped navigation.
- [x] Blue card graphs visible for observed activity; empty histories stay honest;
  reduced motion respected.
- [x] Full pinned formatting, serial all-target tests, and Clippy pass.
- [x] Independent review findings addressed.
