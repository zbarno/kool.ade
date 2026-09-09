# Packet

A native desktop planner with a conversation and a living specification for your git repository.

Run with `cargo run --offline` (once dependencies have been downloaded).

## Slow local models

Planning turns have a **two-hour timeout** by default. Long pauses without output do not end a turn; the Cancel button remains available.

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
(default two hours). Harmless title or purpose paraphrases retain the outline's
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
