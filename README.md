# Packet

A native desktop planner with a conversation and a living specification for your git repository.

Run with `cargo run --offline` (once dependencies have been downloaded).

## Conversations on the board

Every card has a red activity line, and **All activity** above the board combines
worker, investigation, and conversation updates. Charts show observed updates in
ten-second buckets; quiet periods are flat, not an indication that work stopped.
Card colors identify Task, Question, Ambiguity, Assumption, and Ownership, with a
legend above the columns. Active cards have a heavier outline. Inactive cards
retain their last recorded activity window.

Main Chat handles project-wide planning. Every board item also has a focused
conversation with a highlighted next step and **Send answer** when your input is
needed. Only the latest reply appears by default; select the card title to open
a larger input and expandable **Conversation history**. Routine updates say
**No reply needed**, with **Add context** available for optional follow-ups.
Both surfaces share the same draft and messages. **Stop reply** cancels a running response.

Task conversations use the item's durable content, referenced specification
sections, related tasks, and current implementation state. They do not inherit
Main Chat or other task histories, and cannot start project interviews or generate
task batches. Validated decisions update the shared planning artifacts. Completed
questions remain available on the board with their outcomes and conversation.

Histories persist per project under `~/.packet/projects/<slug>/task-conversations.json`
(or `PACKET_HOME`), separately from Main Chat. Shared resolved-item outcomes live
in `planning/resolved-items.json` and are checkpointed in Git. A failed history
save keeps the agent reply visible and offers **Retry saving conversation**;
keep the window open until the retry succeeds.

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

**Auto mode is enabled by default** on the Task stories board. Click
**Implement & continue queue** (or **Resume implementation**) to start. Packet
implements the selected story in its preserved worktree, verifies it, and merges
it into origin's default branch (`main` or `master`) without creating a PR. It then
starts the next unfinished story in the current batch, in order, once its linked
dependencies are Done. Generating stories alone does not start implementation.

Auto publication uses a separate integration worktree based on the latest remote
default branch. Packet squash-merges the task there, reruns its checks, asks the
agent to resolve conflicts or fix integration failures when needed, and publishes
one atomic commit with a normal fast-forward push. A concurrent remote change
triggers another integration against the new base. Failed verification cannot
advance the remote branch or the queue. A lost push response is recovered by
checking the saved commit against the remote history. Clean connected checkouts
on the default branch are fast-forwarded; dirty or divergent checkouts remain
untouched. Uncommitted drafts are never copied into task worktrees.

Disable **Auto mode** to stop queue advancement after the current task and use
pull requests for future implementations. PR mode starts from the connected
branch using the latest compatible remote commit, pushes a task branch, and
creates or reuses a GitHub PR. It requires an authenticated `gh` CLI; Auto mode
requires Git commit identity and push access, but does not invoke `gh` for
publication. Branch protection and unavailable credentials remain real blockers.

Queue settings and the current task persist under Git's private metadata, so
reopening a running queue resumes it. Cancellation pauses the queue. A terminal
failure also pauses it with the worktree and diagnosis preserved; Resume restarts
recovery. Queue, ticket, and publication locks prevent competing Packet windows
from publishing or advancing the same work concurrently.

Packet automatically retries harness failures, including an empty final response.
The Pi event reader can recover a completed final message from `agent_end`.
Implementation agents also write a per-attempt JSON report file before responding;
Packet can recover that report if the CLI loses its final message, and still
validates the report and reruns verification before committing anything.

Report errors and verification failures each receive three normal corrections.
When these are exhausted, two root-cause repair attempts ask the agent to fix the
preventable cause in the current worktree and add regression coverage. Repeated
harness failures similarly escalate to a repair request, with at most six calls
that fail at the harness boundary. Recovery shares the original time budget and
preserves cancellation. Valid explicit blockers, inability to run the agent,
exhausted recovery, and expired budgets still stop safely; recovery does not bypass
checks, credentials, or branch protection. Task changes and verified recovery
fixes are committed together atomically. Response files, failure history, and
verification evidence remain under `packet-implementations`; Pi event streams
remain under `packet-harness` in Git metadata.

Verification commands run in independent POSIX `/bin/sh` processes starting at
the worktree. `$PACKET_WORKTREE` supplies its absolute path even after `cd`;
use `"$PACKET_WORKTREE/Cargo.toml"` for temporary-fixture checks. Shell variables
do not carry between commands. Prompts require executable commands, assertions,
and propagation of failures rather than pipelines that hide exit codes.

The Kanban board groups tasks into To do, In progress, In review, Needs attention,
and Done. Existing PRs are checked every minute while connected: merged PRs become
Done, closed PRs need attention, and reopened PRs return to review. Failed checks
retain the last confirmed state. Task and queue state stay outside tracked story
documents. Previously published tasks are not implemented again.

Planning questions and implementation tasks share the board. Select an item's
title to open its details across the board panel; task specifications and the
main specification use a white paper surface. Task cards show the latest output
and an active-worker indicator. **View all activity** opens a larger viewer with
thoughts, tool commands and results, elapsed run time, and a graph of observed
activity updates in ten-second buckets. These counts are stream updates, not
estimated tokens. Older activity records without timing remain readable.

Task activity is stored privately and stays separate from the project-manager
conversation. The activity viewer follows new output until you scroll back;
closing it returns to the item or board you were viewing.

### Living specification contract

The planner maintains one coherent current specification with the thirteen ordered
sections defined in [the authoring policy](docs/living-specification-policy.md).
Material revisions preserve stable identifiers, explicitly supersede decisions,
separate confirmed intent from repository observations, and retain the accepted
acceptance bar. Git carries detailed history; revision notes stay compact.

Every planning turn receives this policy. Changed specification responses must
have the required title and section structure before any artifacts are written.
Existing documents remain readable; their next material revision must use the new
layout. Structural validation does not certify factual accuracy or semantic
preservation. Task batches retain their frozen approved specification snapshots.
