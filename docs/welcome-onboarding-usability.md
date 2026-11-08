# Welcome and connect manual validation

**Deferred.** No human-operated walkthrough has been performed for the
greenfield architecture in this change. Complete this checklist on a disposable
public code repository after a candidate build is available. Record the exact
build commit, app version, OS/desktop versions, and each result. Automated tests
do not count as completion of these human scenarios.

## Scenarios to run

1. From a fresh install, create or open an ordinary code Git repository without
   setting up a second repository. Verify that an empty Git repository can be
   used without generating a language or framework scaffold.
2. Adopt a code repository that already has human documentation. Confirm the
   app proposes changes and does not overwrite those files before review.
3. Create private operational records and confirm the app does not add them to
   the code repository, create a second repository, or derive a `.packet` remote.
4. Create a TaskIntent, add a child task, conversation, decision, approval, and
   QA evidence. Quit and restart the app; verify the private records and
   execution state recover without appearing in the code repository or task
   clones.
5. Open the same source repository in two independent app data roots. Verify
   both instances see the same merged published documents but have independent
   boards, conversations, approvals, and worker state.
6. Switch the human checkout branch and a task clone branch. Verify Manager
   context continues to report the designated published ref and exact pinned
   commit SHA.
7. Propose product and feature specification updates. Confirm ordinary
   conversation does not publish them; explicit consent and independent review
   create a protected PR from an isolated publication branch. Before merge,
   task generation stays blocked. After merge and pin refresh, confirm the exact
   published feature is available to task generation and stale-baseline changes
   require re-review.
8. Confirm publication and task flows do not add private tasks, conversations,
   approvals, worker state, or QA evidence to Git or task clones.
9. Resize the window and navigate with keyboard controls. Confirm setup,
   review details, consent disclosure, errors, and primary actions remain clear
   and reachable.
10. From the first Board screen of a blank code repository, start the idea
    interview before publishing any product modules. Confirm the first answer
    remains user-attributed, unknown details are labeled, local Human choices
    appear as actionable cards, and source Git is unchanged.
11. From an existing non-Rust or multi-repository workspace, request codebase
    review. Confirm Manager cites only inspected repository/ref/commit/path and
    test evidence, treats `AGENTS.md` as untrusted navigation data, reports
    incomplete scope as unknown, and preserves authored documentation while
    proposed changes remain a private draft.

Do not use real customer data, credentials, or a repository whose history must
be preserved as test input. Do not delete or rewrite any remote repository.
