# Current CI test inventory (2026-11-01)

This inventory records the exact `main` revision `5cc76aa148dfd06fe5850befd89311debd1a84a4` from [required quality run 41352586882](https://github.com/zbarno/kool.ade/actions/runs/41352586882). It is separate from the historical [full-suite inventory](full-suite.md), which describes an older source tree.

## Current results

- The broad all-target Nextest run discovered 3,730 test cases: 3,583 passed, 130 were excluded by the selector, 17 were ignored, and none failed. The run took 465.548 seconds.
- The companion [current CSV](current-full-suite-2026-11-01.csv) lists all 3,730 discovered cases with source mapping, test-name intent, CI outcome and skip reason, duration, and dependency/category signals. Signals and names are inventory metadata, not reviewed semantic judgments.
- The separate Git/implementation job selected 129 cases; the rootless queue-concurrency job selected and passed its one E2E case. The isolated daemon-recovery job also passed. These selectors and job boundaries are defined in [CI](../../.github/workflows/ci.yml).
- The downloadable JUnit artifacts are retained by GitHub for 14 days under `nextest-inventory-41352586882-1`, `nextest-implementation-41352586882-1`, and `nextest-queue-41352586882-1`.

## Scope and refresh

The 3,730-row CSV is the broad job's test discovery and outcome inventory, not a semantic audit and not the sum of unique tests across every CI job. The implementation, daemon-recovery, and queue jobs remain separately selected. In particular, the broad selector excludes most `core::implementation::tests` and the dedicated queue scenario; the implementation selector covers the former, and the rootless queue job runs the latter once. Exact Nextest selectors from `.github/workflows/ci.yml`:

```text
broad all-target job: not (test(/^core::implementation::tests::/) and not test(/docker_lifecycle/)) and not test(/^app::root::board_tests::queue_execution::concurrent_workers::auto_queue_runs_independent_tasks_past_review_and_during_planning$/)
implementation job:  test(/^core::implementation::tests::/) and not test(/docker_lifecycle/)
queue job:           test(/^app::root::board_tests::queue_execution::concurrent_workers::auto_queue_runs_independent_tasks_past_review_and_during_planning$/)
```

To refresh the row inventory, use a successful required CI run and its broad Nextest JUnit artifact at the same commit, then run `python3 scripts/generate-test-inventory.py --junit <nextest.xml> --repo . --output docs/test-inventory/current-full-suite-<date>.csv`. The generator can also accept `--nextest-json` when a matching list is available. Keep the exact revision, run URL, selectors, outcomes, skip reasons, and durations together. Do not relabel `full-suite.md` or `full-suite.csv` as current, and do not compare timing across revisions without disclosing suite growth and job topology.
