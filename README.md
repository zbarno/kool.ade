# Packet

A native desktop planner with a conversation and a living specification for your git repository.

Run with `cargo run --offline` (once dependencies have been downloaded).

See [the artifact layout and cleanup review](docs/artifact-layout.md) for current
specifications, Packet working artifacts, configuration, and published decisions.

## Conversations on the board

Conversations stay in a persistent tabbed panel to the left of the Kanban (above it
on narrow screens). **Main Chat** is selected initially and cannot be closed.
**Open conversation** on a card adds and focuses that item's tab, or focuses its
existing tab. Each additional tab has a close button; closing the selected tab
returns to Main Chat without clearing history or drafts or cancelling a running
reply. Inline replies, task details, and the tab share the same conversation and
draft. Only the selected chat is rendered, with no additional native windows.
Clicking a card title still opens task details, descriptions, and workflow actions.
Overall status and the combined activity graph stay in the top bar. Queue controls
are available in the **Workspace → Settings…** modal.

Every card has a red activity line, and **All activity** in the top bar combines
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
On cards, the reply sits directly beneath the question, ahead of metadata and
activity. Expanded conversations use a four-line input and visually separate
your messages from Packet's replies; timestamps remain available on hover.

Opening a card creates a focused task workspace: its title, category, and status
come first, followed by the current question, decision, or implementation action.
Your latest answer stays visible while Packet responds. Task descriptions,
acceptance criteria, background evidence, history, activity, and technical details
remain available in expandable sections below the current interaction.

Sending an answer moves an otherwise unstarted item to **In progress**. Saved
conversation history keeps it there after reopening Packet. Failed or interrupted
replies show **Needs attention** until retried; explicit blockers, review, and
completed states take precedence. Discussion alone never marks a task Done.

An architectural decision record is created when a user approves a planning choice
that the planner identifies as durable and consequential. Routine choices do not
create ADRs, and task completion does not create one. Implementation reports,
verification commands, and acceptance evidence stay in the separate implementation
evidence directory.

Task conversations use the item's durable content, referenced specification
sections, related tasks, and current implementation state. They do not inherit
Main Chat or other task histories, and cannot start project interviews or generate
task batches. Validated decisions update the shared planning artifacts. Completed
questions remain available on the board with their outcomes and conversation.

Histories persist per project under `~/.packet/projects/<slug>/task-conversations.json`
(or `PACKET_HOME`), separately from Main Chat. Shared resolved-item outcomes live
in `.kool-ade-packet/planning/resolved-items.json` and are checkpointed in Git. A failed history
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
.kool-ade-packet/planning/tasks/saved-searches/
  README.md
  specification.md
  001-persist-named-search-filters.md
  002-build-the-saved-search-picker.md
```

Stories lead with the specific problem the ticket solves, why it matters, and the
observable outcome delivered by that ticket alone. The product context remains in
the linked approved specification. Each story carries the context, affected area,
steps, acceptance criteria, tests, and verification needed for its own work. Design,
edge cases, and rollout details appear when that issue calls for them. Open
**Task stories** in the document panel to review the latest batch.

Readiness survives reopening the repository. Each validated story is saved immediately
under the feature directory and appears in the Task stories panel during generation.
The batch index shows **In progress — N of M stories saved** until the entire batch
passes coverage and dependency validation. Cancellation or failure preserves saved
stories and the specification. Private checkpoints let an unchanged plan resume at
the unfinished story without rewriting completed files. User edits to saved stories
stop generation for review instead of being overwritten. A
changed plan, configuration, or tracked checkout starts a fresh run. Immutable
attempt and validation records are kept beside checkpoints for diagnosis. Task detail
scales with its actual complexity: a small change can stay concise, while a risky
migration can include the relevant design, compatibility, failure, and recovery work.
Required task meaning and verification are checked without minimum word counts;
irrelevant optional sections stay out of the generated story. Earlier batches remain
intact; regenerated batches use directories
such as `saved-searches-02`. Task generation creates plans, not implementation changes.

## Implement a ticket

**Build approved changes automatically** is enabled by default; **Publish verified
changes automatically** is off. Click **Implement & continue queue** (or
**Resume implementation**) to start approved work. Packet builds in preserved,
isolated worktrees and verifies each task. Verified work stays local until you
choose to share it for review or enable automatic publication. Dependencies must
be merged before a dependent task starts; unrelated ready tasks can use other
worker slots. Each worker has its own worktree, progress, cancellation, and
verification. Generating stories alone does not start implementation.

**Workspace → Settings… → Concurrent tasks** controls the pool (default **3**,
range **1–8**). The same settings separately control automatic planning,
implementation, publication, and whether to wait for independent project checks.
Lowering the limit does not interrupt running workers. You can also start another
eligible task manually while workers are active.

When **Publish verified changes automatically** is enabled, publication is
serialized per target repository, even when implementation runs concurrently.
Verified tasks show **Waiting to merge** until the coordinator
can integrate them. Publication uses a separate integration worktree based on the latest remote
default branch. Packet squash-merges the task there, reruns its checks, asks the
agent to resolve conflicts or fix integration failures when needed, and publishes
one atomic commit with a normal fast-forward push. A concurrent remote change
triggers another integration against the new base. Failed verification cannot
advance the remote branch or unblock dependent tasks. A lost push response is recovered by
checking the saved commit against the remote history. Clean connected checkouts
on the default branch are fast-forwarded; dirty or divergent checkouts remain
untouched. Uncommitted drafts are never copied into task worktrees.

Completed tasks tidy themselves up after publication or a merged PR is confirmed.
Packet checks that the completion commit is still in the remote base branch,
then removes clean task and integration worktrees, including their ignored build
output. Changed, unverified, locked, or mismatched worktrees are preserved.
Verification reports remain in the implementation evidence directory, and private
Git refs retain verified commits. Task branches are deleted only when Git can
safely delete them; squash-merged branches may remain without their build output.

Cleanup runs off the UI thread. Pending cleanup, including tasks completed by
older Packet versions, retries on project open and every minute while the project
is open. A cleanup failure leaves the task Done and shows **Cleanup needs
attention** with copyable details on its card. Resolve the reported cause; the
next maintenance pass retries automatically. In-flight, unpublished, and failed
tasks retain their worktrees for resume. No cleanup runs while Packet is closed.

For a one-time maintenance pass without opening the desktop, run
`cargo run --offline --example cleanup_completed -- /absolute/planning/repository`.
This uses the same locks, publication checks, and preservation rules as the app.

Automatic planning investigates Agent-owned questions; it does not create tasks
or start builds. Automatic building continues only explicitly approved work.
Automatic publication is a separate permission and is disabled by default.
Sharing a verified task for review requires an authenticated `gh` CLI. Automatic
publication requires Git commit identity and push access, but does not invoke
`gh`. Branch protection and unavailable credentials remain real blockers.

Queue settings and in-flight tasks persist under Git's private metadata, so
reopening a running queue resumes unfinished work. Stopping a task pauses new
starts and cancels only that worker; other running workers are preserved. A terminal
failure blocks that task and its dependents but lets unrelated work continue.
Its worktree and diagnosis remain available; Resume restarts recovery.
Automatic building also recognizes previously parked publication-divergence and
explicit verification-only no-change failures and schedules one automatic resume. That
retry budget persists across restarts, and explicit cancellation suppresses it.
Divergent local history is preserved: Auto workers use the remote base and repair
integration conflicts in isolated worktrees before rerunning verification.
Verification-only contracts that explicitly require zero repository changes can
complete with verified evidence without creating an empty commit or PR.
Queue, ticket, and publication locks prevent competing Packet windows
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
verification evidence remain locally under `.kool-ade-packet/implementation/`,
which Git ignores. Pi event streams remain under `packet-harness` in Git metadata.

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

The planner maintains one coherent current specification. It has six required
concepts—Overview, Users and Outcomes, Current Capabilities, Architecture and
Constraints, Decisions, and Quality and Acceptance—plus concise optional modules
when the project needs them. The authoring rules live in
[the planner policy](docs/planner-policy.md). Material revisions preserve stable
identifiers, explicitly supersede decisions, separate confirmed intent from
repository observations, and retain the accepted acceptance bar. Git carries
detailed history; revision notes stay compact.

Every planning turn receives this policy. Responses use strict operation-specific
schemas, and changed documents must have a title before any artifacts are written.
Existing numbered specifications migrate into the six required concepts while
retaining useful project-specific modules. Structural validation does not certify
factual accuracy or semantic preservation. Task batches retain their frozen
approved specification snapshots.
