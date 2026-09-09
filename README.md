# Packet

A native desktop planner with a conversation and a living specification for your git repository.

Run with `cargo run --offline` (once dependencies have been downloaded).

## Slow local models

Planning turns have a **twelve-hour timeout** by default. Long pauses without output do not end a turn; the Cancel button remains available.

To allow more time, set a positive timeout in seconds when launching Packet. For example, ten hours:

```sh
PACKET_TURN_TIMEOUT_SECS=36000 cargo run --offline
```

Invalid or zero values use the default. The timeout is fixed when each turn begins; already-running turns keep their original deadline. Restart Packet with the rebuilt binary to use the new default.

## From interview to task stories

The planner first clarifies the product or feature's goal, intended users, desired
outcome, scope, exclusions, constraints, and success criteria. Once the specification
is ready, it asks whether to proceed. Click **Generate task stories** or reply **yes**
to that offer. You can also keep discussing the plan; generation requires a fresh
readiness assessment after further changes.

Generation builds an ordered outline, then gives the local model a separate response
for each detailed story. Dependencies are passed forward so the stories agree on
interfaces. Invalid or incomplete responses receive precise repair feedback, with
at most three attempts per outline/story under the same configured turn timeout
(default twelve hours). Harmless title or purpose paraphrases retain the outline's
stable identity instead of failing generation.

Example output:

```text
planning/tasks/saved-searches/
  README.md
  specification.md
  001-persist-named-search-filters.md
  002-build-the-saved-search-picker.md
```

Stories lead with the specific problem the ticket solves, why it matters, and the
observable outcome delivered by that ticket alone. The product context remains in
the linked approved specification. Stories also include user stories, scope references, dependencies,
implementation context, technical contracts, affected files, ordered implementation
steps, observable acceptance criteria, test plans, edge cases, verification commands,
rollout notes, and definitions of done. Open **Task stories** in the document panel
to review the latest batch.

Readiness survives reopening the repository. Each validated story is saved immediately
under the feature directory and appears in the Task stories panel during generation.
The batch index shows **In progress — N of M stories saved** until the entire batch
passes coverage and dependency validation. Cancellation or failure preserves saved
stories and the specification. Private checkpoints let an unchanged plan resume at
the unfinished story without rewriting completed files. User edits to saved stories
stop generation for review instead of being overwritten. A
changed plan, configuration, or tracked checkout starts a fresh run. Immutable
attempt and validation records are kept beside checkpoints for diagnosis. Stories
must meet section-specific detail checks and a 450-word minimum; the prompt asks
for task-specific detail, typically 700–1400 words, rather than filler. These checks
validate structure and depth, not the correctness of every implementation decision.
Earlier batches remain intact; regenerated batches use directories
such as `saved-searches-02`. Task generation creates plans, not implementation changes.

## Implement a ticket

Select a numbered story in **Task stories** and click **Implement**. This starts Pi
in a dedicated Git worktree on a stable `packet/<ticket>` branch. The worktree starts
from the connected checkout's current committed branch; uncommitted changes in the
original checkout are not copied or modified. Worktrees live in a sibling
`.packet-worktrees` directory, with execution state and verification evidence under
Git's private `packet-implementations` metadata directory.

**Resume implementation** reopens the same worktree. Pi reviews its status, diffs,
commits, repository instructions, and ticket dependencies before continuing. Cancel,
application exit, failed verification, and unavailable GitHub preserve the worktree.
Changed ticket text requires review and a revised ticket; it cannot silently change
an existing implementation's scope. A per-ticket lock prevents concurrent writers
from separate Packet instances.

Packet requires a complete implementation report with evidence for every listed
acceptance criterion, reruns the reported verification commands, checks the diff,
and commits the result. It then pushes to `origin` and creates a GitHub pull request
against the branch selected when implementation began. The app needs Git commit
identity, push access to `origin`, and an authenticated GitHub CLI (`gh auth login`).
It reuses an existing open PR and exposes **Open PR** once publishing succeeds.
Publishing failures can be retried without repeating a verified implementation when
the worktree and commit are unchanged. PRs are never automatically merged.

Verification combines the model's acceptance evidence with actual command results;
it does not replace human code review. Pi has normal local coding-tool access; the
worktree is isolation for Git changes, not an operating-system sandbox.
