# Greenfield Cutover Validation

Automated validation covers the Git/storage boundary and local application
flows. These manual checks remain a release gate and have not been performed:

- On a clean installation, create and reopen a project from a code Git
  repository without provisioning a paired repository or importing `.packet`
  state. Confirm the app reports missing/pending specs clearly.
- Connect an existing code repository with an unrelated `packet` remote and
  confirm connection does not fetch, inspect, or mutate that remote.
- In the native desktop UI, create a TaskIntent with child tasks, conversation,
  decision, approval, and QA evidence; restart the app and confirm all remain
  local while the code repository stays clean.
- Open the same published code repository from two separate Kool.ad/e data
  roots. Confirm both show the same pinned merged specs and have independent
  boards, conversations, approvals, and execution history.
- Review a local specification proposal, explicitly consent to publication,
  and confirm the isolated source-repository branch/PR flow, protected merge,
  pin refresh, and stale-pin rejection.
- In Project Settings, register a secondary repository by selecting a checkout
  with the matching remote, restart the app, and verify the private mapping is
  retained. Create a task targeting that repository with explicit source and
  destination branches; confirm its isolated clone uses those branches while
  the primary published-spec pin stays fixed when the human checkout branch
  changes.
- Run a native end-to-end task through isolated clone execution, independent QA,
  human review, and guarded PR publication. Record each gate's real evidence.

Do not mark these checks complete based on unit tests, mocks, or successful
compilation. Keep issue #484 or a dedicated release-gate issue open until they
are performed and their evidence is recorded.
