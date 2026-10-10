# Workspace UI review

The UI uses neutral surfaces, blue actions and measured activity, and a small
red brand accent. Shared text, spacing, colors and scrollbars are applied through
`ui::theme::apply` to both the desktop application and the screenshot harness.

## Layout decisions

- Header content participates in normal layout. The project title cannot cover
  workspace controls. Git state and live activity occupy separate rows.
- Cards measure their content; prompts, actions and charts cannot spill into the
  next card. Child buttons retain their own interaction targets.
- Blue card charts show observed ten-second buckets with a translucent area
  fill and a highlighted latest update. Missing updates stay at zero. Live
  charts include the current bucket; settled histories remain fixed. Reduced
  motion stops the highlight pulse.
- Desktop details place conversation beside state and actions. Narrow details
  use one continuous scrollable document. Plan alternatives stack when narrow.
- Settings use grouped navigation and a bounded reading width. Tool selection,
  stakeholder forms and branch selectors adapt to narrow windows.
- The compact specification view starts with the document; the conversation
  can be expanded explicitly.

## Reproduce the rendered review

The ignored GPU review uses `KooladeApp::paint_screen`, the production egui
painters and the same eframe GPU renderer. It supplies synthetic project,
people, tool and telemetry data, sends pointer/scroll events, and never ticks
workers or submits a plan. Screenshots are review evidence, not pixel snapshots
that lock the visual design in place.

```sh
KOOLADE_HOME="$(mktemp -d)" \
KOOLADE_UI_REVIEW_DIR=/tmp/koolade-ui-review \
cargo +1.98.1 nextest run --locked --lib --run-ignored only \
  -E 'test(/render_workspace_review_gallery/)'
```

The gallery covers 1600×900, 1280×720, 900×720, 360×720, 360×480 and 1280×480:
board and workspace menu, implementation details and full activity, each of the
seven Settings pages, New Task, specification, planning/question details,
feature comparison, setup recovery, reference import, MCP configuration,
welcome, and standalone coding-tool configuration. Long views include scrolled
captures to review the controls below the fold.

Representative captures (synthetic data): [board](ui-review/board.png),
[Settings](ui-review/settings.png), [task details](ui-review/task-details.png),
[compact task details](ui-review/compact-task-details.png), and
[Settings in a short window](ui-review/short-window-settings.png).

This review checks the production painter through an offscreen GPU renderer.
It does not establish external coding-tool availability or exercise live workers.

Targeted interaction tests cover card growth and child clicks, title/control
separation, current activity buckets, planning history, branch selection,
compact settings navigation, plan adoption, and task navigation.

Run the normal pinned format, serial all-target test and Clippy gates after the
review. Finish compilation before running the full test suite: its diagnostics
test relaunches its own executable and must not overlap a rebuild of that file.

## Final result — 2026-10-07

All pinned gates passed: formatting, 1,034 tests across all targets, and Clippy
with warnings denied. Two opt-in tests were ignored by the normal suite; the GPU
gallery was run separately and passed with 168 captures. Independent review
completed with no blocking findings after the short-window Settings fix.
