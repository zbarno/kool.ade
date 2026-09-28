```markdown
# Packet Task-Centric UX and Architecture Remediation Plan

## Status

Proposed follow-up implementation plan against current `master` after the hardening review.

Current reviewed baseline:

`73f00c9bae081faed70bfa32124d418162d79fb3`

This plan intentionally shifts Packet away from a chat-first planning application and toward a **task-centric asynchronous planning workspace**.

The core product change is:

> The Kanban board becomes the primary interaction surface. The user creates work through **New Task**. Packet performs planning and investigation asynchronously. Whenever Packet needs the user, it creates a clear **Needs Attention** item instead of interrupting them with chat questions.

The main planning agent still exists, but becomes an orchestrator working behind the board rather than a chatbot the user must continually converse with.

---

# 1. Product Direction

Packet should optimize for a nontechnical person who wants to build software with AI without needing to understand software architecture, implementation mechanics, Git, or agent orchestration.

The user should primarily need to understand:

1. What am I trying to accomplish?
2. What is Packet doing?
3. What needs my attention?
4. What does Packet recommend?
5. What happens if I choose each option?
6. What happens if I do nothing?
7. What is ready to build?
8. What is currently being built?
9. What changed?
10. Why was it built this way?

Everything else is supporting machinery.

---

# 2. Primary UX Model

The application should revolve around:

```text
New Task
   │
   ▼
Planning Work Item
   │
   ├── Agent investigates independently
   ├── Agent updates specification
   ├── Agent creates internal work items
   │
   └── Agent creates Needs Attention items when user input is required
               │
               ▼
        User responds asynchronously
               │
               ▼
        Planning continues automatically
               │
               ▼
          Ready for approval
               │
               ▼
          Implementation
               │
               ▼
         Verification
               │
               ▼
        Reconciliation
               │
               ▼
              Done
```

The user should not need to shepherd the agent through a long sequential conversation.

---

# 3. Core Product Principles

## 3.1 Kanban Is the Interaction Model

All durable user-agent interaction should have a board identity.

This includes:

- questions;
- decisions;
- ambiguities;
- assumptions requiring review;
- approvals;
- plan choices;
- implementation blockers;
- verification failures requiring action;
- missing configuration;
- ownership questions;
- project setup issues;
- agent-discovered problems.

Do not leave actionable user requests only in:

- chat history;
- toast messages;
- logs;
- progress text;
- transient dialogs.

Transient informational messages may still use toasts.

Anything requiring future user action belongs on the board.

---

## 3.2 Main Chat Is No Longer a Primary Product Surface

Remove **Main Chat** from the normal user workflow.

The main agent continues to exist.

Its responsibilities become:

```text
observe
plan
investigate
coordinate
update specifications
identify uncertainty
create board items
resolve agent-owned work
watch implementation/reconciliation
surface meaningful user decisions
```

It should not depend on maintaining an open conversational session with the user.

### Existing Main Chat History

Do not immediately delete historical Main Chat persistence.

For this iteration:

- remove Main Chat from the primary UI;
- retain old stored history for migration/debugging;
- do not use old Main Chat history as authoritative project truth;
- do not require users to interact with it;
- do not create new user workflows dependent on it.

A later cleanup can remove obsolete persistence once the board-centric flow is proven.

---

# 4. New Task Becomes the Primary Entry Point

Add a prominent:

```text
+ New Task
```

action.

This should be available from the main board without opening another major navigation surface.

The first interaction should be extremely small.

---

# 5. New Task Types

The user selects one of four initial task types.

```text
Feature
Bug
New Project
Question
```

Do not create a giant task-creation wizard.

The initial interaction should approximately be:

```text
What do you want to do?

[ Feature ]
[ Bug ]
[ New Project ]
[ Question ]

Describe it:

[ __________________________________________ ]

                         [ Create Task ]
```

Optional additional context can be added afterward from the task itself.

---

# 6. Task Type Semantics

## Feature

Use when the user wants new behavior.

Examples:

```text
Add dark mode.
Allow users to compare plans.
Add Stripe billing.
```

Packet should:

1. create a planning work item;
2. inspect relevant product/repository context;
3. create a Draft change specification;
4. identify uncertainty;
5. resolve agent-authority questions itself;
6. create Needs Attention cards for actual user decisions;
7. update the specification as decisions resolve;
8. mark the change ready;
9. surface approval through the board.

---

## Bug

Use when existing behavior is wrong.

Examples:

```text
The dialog gets cut off.
Login occasionally hangs.
The task card shows stale status.
```

Packet should first investigate.

The initial planning task should attempt to determine:

```text
expected behavior
observed behavior
likely affected area
impact
reproduction evidence
proposed fix
risk
verification
```

Do not force the user through a full feature interview when repository evidence can answer the questions.

If the fix changes intended product behavior, create a change specification.

If it is purely restoring already-specified behavior, a concise corrective change specification is sufficient.

Bug fixes should not produce giant planning documents.

---

## New Project

Use to bootstrap planning for a new product.

Packet should progressively establish:

```text
problem
target users
desired outcome
scope
important constraints
major product behaviors
major architectural constraints
risks
unknowns
```

Do not conduct this as a 30-question synchronous interview.

Instead:

1. create the project planning task;
2. establish everything that can reasonably be inferred;
3. generate multiple independent Needs Attention items;
4. allow the user to answer them asynchronously;
5. update the product specification continuously;
6. surface additional dependent questions only when necessary.

---

## Question

Use when the user simply wants to ask Packet something.

Examples:

```text
Why did we choose SQLite?
What happens if we change this API?
How does authentication currently work?
Should this be one service or two?
```

The task itself may be answered directly.

If no decision or follow-up is required:

```text
Question
  ↓
Agent investigates
  ↓
Answer recorded
  ↓
Done
```

If the question reveals a decision or problem, Packet may generate related Needs Attention or planning items.

This replaces the need for a generic Main Chat.

---

# 7. Introduce a Proper Planning Work Model

The current:

```rust
planning_work::Work
```

is becoming a central product object and should no longer remain a loose structure driven by numeric columns.

Replace or evolve it into a versioned typed model.

Conceptually:

```rust
struct PlanningWorkItem {
    schema_version: u32,
    uid: String,
    display_id: String,
    kind: PlanningWorkKind,
    title: String,
    request: String,
    status: PlanningWorkStatus,
    feature_id: Option<String>,
    created_at: ...,
}
```

Possible kinds:

```rust
enum PlanningWorkKind {
    Feature,
    Bug,
    NewProject,
    Question,
}
```

Possible statuses:

```rust
enum PlanningWorkStatus {
    Planning,
    NeedsAttention,
    Ready,
    InProgress,
    Done,
    Blocked,
    Cancelled,
}
```

Use only the statuses actually needed.

Do not duplicate implementation-task lifecycle unnecessarily.

---

# 8. Remove Numeric Board State

Avoid logic such as:

```rust
column = 1
column = 3
column = 4
```

Introduce a typed board projection.

Conceptually:

```rust
enum BoardColumn {
    Planning,
    NeedsAttention,
    Ready,
    InProgress,
    Done,
}
```

The persisted semantic state should drive the board.

UI column indexes should remain presentation details.

A future column reorder must not change workflow behavior.

---

# 9. Add Stable Parentage Between Work and Interactions

All questions, decisions, tasks, and implementation work generated because of a New Task should be traceable back to that request.

Add a stable relationship such as:

```text
work_uid
```

to relevant generated artifacts where needed.

For example:

```text
PlanningWorkItem
   uid = work-123

Feature
   originating_work_uid = work-123

OpenItem
   work_uid = work-123

TaskBatch
   work_uid = work-123
```

Do not use titles or filesystem paths as this relationship.

This lets Packet answer:

```text
Why does this question exist?
Which user request caused this implementation task?
What work is blocked by this answer?
```

---

# 10. Needs Attention Becomes the Universal User-Interaction Pattern

This is the most important UX requirement in this plan.

Whenever Packet needs something from the user, the result should appear in:

```text
Needs Attention
```

The card must be understandable without reading conversation history.

---

# 11. Required Needs Attention Content

Every Needs Attention item MUST clearly answer:

```text
What is the issue?

Why does it matter?

What does Packet recommend?

Why does Packet recommend that?

What happens if I choose each option?

What happens if I defer?

What work is blocked?

Can I change this later?
```

The user must not have to understand source code to answer.

---

# 12. Reuse the Existing Decision Brief

Do not invent another overlapping abstraction if unnecessary.

The existing decision-brief model already supports much of this:

```text
question
whyNow
recommendation
confidence
options
benefits
costs
risks
ramifications
reversibility
deferConsequence
evidence
adrAssessment
```

Build the new Needs Attention UX around this structure.

Expand it only where there is a concrete missing field.

---

# 13. Needs Attention Rendering

A card should initially show a concise summary.

Example:

```text
Database choice needed

Packet recommends SQLite.

Why:
Your app is local-first, has one writer, and does not currently
need a network database.

Impact:
This choice affects how user data is stored and backed up.

[ Use SQLite ]
[ Use Postgres ]

[ Write my own answer… ]
```

Opening the card should reveal additional detail.

---

# 14. Detailed Needs Attention View

Use a consistent structure.

## Issue

Plain-language explanation.

Example:

```text
Packet needs to know where project data should live.
```

## Why this matters

Explain the consequence.

Example:

```text
Changing this later would require a data migration, so choosing now
prevents implementation work from being thrown away.
```

## Packet recommends

Highlight the recommended option.

Example:

```text
SQLite
```

Then explain:

```text
Why Packet recommends this:
The current requirements only require one local application instance,
and SQLite avoids running another service.
```

## Options

Each option should be individually actionable.

Example:

```text
SQLite

Good for:
- simple deployment
- local-first use
- low operational overhead

Tradeoffs:
- harder to support many simultaneous writers later

Reversible?
Yes, but migrating existing data would require work.

[ Choose SQLite ]
```

---

# 15. Freeform Input Must Always Remain Available

Even when Packet provides buttons, always provide:

```text
Something else…
```

or:

```text
Write my own answer
```

The user should never be trapped inside model-generated options.

The agent should interpret the freeform response and update the item accordingly.

---

# 16. Recommendations Are Advice, Not Authority

For Human items:

```text
recommendation != selection
```

Packet may strongly recommend an option.

It may explain why.

It may rank consequences within the specific decision.

But the Human decision is not resolved until the user responds.

---

# 17. Stop Asking One Question at a Time

The current planner policy contains assumptions based on chat interaction such as:

```text
ask at most one Human/Blocking question per response
```

That is incompatible with the new asynchronous UX.

Replace it.

---

# 18. New Planning Question Policy

The planner should:

1. investigate first;
2. resolve all safe Agent-authority items itself;
3. identify every currently-known independent Human/Review decision;
4. create separate Needs Attention items for them;
5. continue any work not blocked by those decisions;
6. surface dependent questions only when their prerequisites resolve.

Example:

```text
Feature planning discovers:

A. User needs to choose storage location.
B. User needs to choose notification style.
C. Authentication design depends on storage choice.

Create now:
A
B

Do not create C yet.
```

This creates useful parallelism without overwhelming the user with speculative questions.

---

# 19. Interaction Frontier

Introduce the concept of an **interaction frontier**.

The user should see all decisions that are:

```text
currently known
meaningful
independent
actionable now
```

Do not surface:

```text
questions whose answer depends on another unresolved question
questions the agent can answer itself
hypothetical questions
low-value implementation details
```

This should dramatically reduce conversational back-and-forth.

---

# 20. User Interaction Should Resume Planning Automatically

When the user resolves a Needs Attention item:

```text
answer
   ↓
persist decision
   ↓
update specification
   ↓
re-evaluate planning task
   ↓
continue independent work
   ↓
possibly create new Needs Attention items
```

The user should not then need to type:

```text
continue
```

or:

```text
what's next?
```

---

# 21. Main Agent Becomes the Board Orchestrator

The main agent still exists.

Its operating model changes.

It should continuously evaluate active planning work for:

```text
missing information
repository discoveries
contradictions
unresolved decisions
failed assumptions
implementation blockers
specification drift
verification issues
new user responses
reconciliation mismatches
```

The response to these events should normally be:

```text
update task
create board item
resolve board item
update specification
```

rather than:

```text
send Main Chat message
```

---

# 22. Durable User Interaction Rule

Introduce a product invariant:

> If Packet needs the user to do something later, that requirement must have a durable board item.

Examples:

```text
choose an option
approve a plan
fix configuration
supply missing information
resolve a mismatch
review an assumption
retry an external prerequisite
```

Do not rely on a toast or chat message.

---

# 23. Approvals Should Become Needs Attention Items

The current feature approval button is useful, but approval should participate in the same interaction system.

When planning is complete, create an approval interaction.

Example:

```text
F12 is ready to build

Packet recommends approving this plan.

What will be built:
...

Important consequences:
...

Decisions already made:
...

Remaining risk:
...

[ Approve ]
[ Request changes ]
[ Ask a question ]
```

This can still invoke the existing typed approval action.

The important change is presentation and board visibility.

---

# 24. Plan Comparison Should Use the Same Pattern

A plan comparison is simply a particularly rich Needs Attention decision.

Example:

```text
Choose implementation plan

Packet recommends Plan B.

Plan A
...
[ Choose Plan A ]

Plan B
...
[ Choose Plan B ]

[ Ask about these plans ]
[ Write another direction ]
```

No special conversational flow should be required.

---

# 25. Implementation Blockers Should Use Needs Attention Too

Examples:

```text
GitHub checks are unavailable.

Packet recommends waiting until the checks are available before publishing.

Impact:
The implementation is complete and preserved locally.
Nothing has been published.

[ Retry checks ]
[ Keep local ]
[ Open details ]
```

Do not expose this only as:

```text
queue.last_error
```

or a toast.

---

# 26. Configuration Problems Should Become Attention Items

This applies directly to the current provider/platform behavior.

Examples:

```text
Packet cannot access the configured model safely.

Packet currently supports private/local HTTP OpenAI-compatible Pi providers.

Recommendation:
Use the configured local vLLM provider.

Impact:
Planning cannot inspect the repository until a supported provider is selected.

[ Open model settings ]
[ Retry ]
```

---

# 27. Make Packet Explicitly Linux-Only for Now

Do not spend engineering time maintaining platforms that are not being used.

Current supported product target:

```text
Linux x86_64
```

This should be stated consistently in:

```text
README
product scope
architecture specification
developer documentation
CI
runtime capability code
```

---

# 28. Remove Misleading Cross-Platform Claims

Remove claims that Packet currently supports:

```text
macOS
Windows
```

unless they are explicitly described as future work.

The README should not suggest supported behavior merely because portions of the source might compile there.

---

# 29. Simplify Runtime Capability Logic

Inspect:

```text
src/harness/runtime_capabilities.rs
```

The current:

```rust
Platform::Linux
Platform::MacOs
Platform::Windows
Platform::Other
```

abstraction is unnecessary if Linux is the only supported target.

Simplify it if doing so reduces complexity.

Preferred model:

```text
Linux
 ├── Bubblewrap available
 │      planning repository access available
 │      implementation available
 │
 └── Bubblewrap unavailable
        Packet setup incomplete
```

Do not maintain fake/test-only product capability branches for unsupported operating systems unless they provide meaningful architectural value.

---

# 30. Bubblewrap Is a Product Prerequisite

For the current product, treat Bubblewrap as a Linux prerequisite.

At startup or project connection, Packet should detect it.

If unavailable, create a clear setup issue rather than quietly producing confusing degraded behavior.

Example:

```text
Packet needs Bubblewrap

Packet uses Bubblewrap to prevent planning and implementation agents from
accessing unrelated files on your computer.

Recommendation:
Install Bubblewrap before allowing repository-aware agents to run.

[ Show setup instructions ]
[ Retry ]
```

It is acceptable to keep context-only behavior internally if useful.

It should not complicate the primary product model.

---

# 31. Keep Current Provider Scope Explicit

Do not generalize provider support during this remediation.

Current secure planning provider support may remain:

```text
private/local HTTP
OpenAI-compatible
Pi provider
```

That matches the current usage.

Document this clearly.

Do not pretend arbitrary hosted Pi providers are supported.

---

# 32. Unsupported Provider Behavior

Do not crash the planning workflow or emit only an error toast.

Create a Needs Attention configuration item.

This item should explain:

```text
what is unsupported
why Packet blocks it
what Packet recommends
what functionality is unavailable
how to fix it
```

---

# 33. Rename Provider Abstractions If Helpful

If the current generic naming implies broader support than exists, prefer something explicit such as:

```text
LocalProviderBridge
```

rather than:

```text
ProviderBridge
```

Only do this if the rename reduces ambiguity.

Do not perform naming churn merely for aesthetics.

---

# 34. Clean Up Plan Comparison Authority

The current comparison feature persists overlapping state in:

```text
ChangeMetadata
workflow.json
```

Define clear ownership.

---

# 35. Plan Comparison Authority Model

`workflow.json` should own the planning process:

```text
generated alternatives
recommendation
comparison transcript
comparison status
selection event
timestamps
discard history
```

The change specification should own accepted product intent:

```text
the adopted plan
why it was selected
relevant ADR reference
```

---

# 36. Remove Long-Term Duplicate Machine State

The application should not require:

```text
workflow.selected_plan == change_metadata.selected_alt
```

forever.

After migration:

- `workflow.json` remains the historical workflow record;
- the change specification contains the adopted plan;
- feature approval fingerprints the resulting feature contract;
- `ChangeMetadata` returns to lifecycle concerns.

Target `ChangeMetadata`:

```text
schema version
stable UID
display ID
change status
```

Avoid turning it into a general feature-data container.

---

# 37. Migration for Existing Comparison Metadata

Add a safe migration.

For feature documents currently containing:

```text
plan_comparison
comparison_history
selected_alt
```

do the following:

1. preserve/adopt the corresponding workflow record;
2. ensure adopted plan is rendered into the specification;
3. preserve historical comparison evidence in workflow state;
4. remove obsolete duplicate metadata fields in the new schema version.

Do not lose existing Plan A/B evidence.

---

# 38. Specification Lifecycle

The board workflow must continue producing useful artifacts.

## For code-changing work

Every meaningful Feature or Bug planning task should result in an appropriate change specification.

The specification should remain concise.

It should answer:

```text
Intent
Current behavior
Desired behavior
Scope
Affected product areas
Requirements
Decisions and assumptions
Acceptance criteria
```

Do not duplicate the full planning conversation.

---

# 39. Product Specification

The product specification remains current product truth.

It should describe:

```text
what the product currently is
what behavior exists
important architecture and constraints
```

It should not accumulate every historical feature discussion.

After implementation and reconciliation:

```text
approved change
    ↓
verified implementation
    ↓
reconciliation
    ↓
affected product modules updated
```

---

# 40. ADR Policy

Keep the current ADR philosophy.

Create an ADR only when the user makes a decision that is:

```text
durable
consequential
likely to matter later
worth understanding historically
```

Examples:

```text
database technology
deployment architecture
authentication boundary
cross-repository ownership
major persistence strategy
```

Do not create ADRs for:

```text
minor UI details
routine bug fixes
implementation transcripts
task completion
```

---

# 41. Questions and Specs

A pure Question task does not need to generate a specification.

A Question that turns into:

```text
change this behavior
```

should offer:

```text
Turn this into a Feature
```

and create a linked planning work item.

---

# 42. Board Read Model

Continue moving complex UI reads into typed view models.

Introduce a cohesive card model rather than expanding `Surface`.

Conceptually:

```rust
struct BoardCardViewModel {
    uid: String,
    title: String,
    kind: BoardCardKind,
    column: BoardColumn,
    status: String,
    summary: String,
    needs_attention: Option<AttentionViewModel>,
    activity: ...,
    actions: Vec<BoardAction>,
}
```

The UI should not reconstruct workflow meaning from multiple application getters.

---

# 43. Needs Attention View Model

Create one explicit UI model for the interaction.

Conceptually:

```rust
struct AttentionViewModel {
    issue: String,
    why_it_matters: String,
    recommendation: Option<RecommendationView>,
    options: Vec<OptionView>,
    defer_consequence: Option<String>,
    blocked_work: Vec<String>,
    reversible: Option<String>,
    evidence: Vec<String>,
    allow_freeform: bool,
}
```

This may be derived from the existing DecisionBrief rather than creating new persisted fields.

Prefer view transformation over persistence duplication.

---

# 44. Progressive Disclosure

Default Needs Attention view:

```text
issue
recommendation
one-line consequence
options
freeform response
```

Expanded view:

```text
why now
benefits
costs
risks
ramifications
reversibility
defer consequence
evidence
technical details
conversation history
```

A nontechnical user should not have to read technical detail to act.

---

# 45. Technical Evidence Should Be Collapsible

Source files, Git commits, test output, and implementation evidence are useful.

They should generally appear under something like:

```text
Technical evidence
```

rather than being mixed into the decision explanation.

---

# 46. Task Conversations Still Exist

Removing Main Chat does not mean removing conversation.

Each work item and Needs Attention item can retain its focused conversation.

Conversation becomes:

```text
context attached to the task
```

rather than:

```text
the application
```

This is much cleaner.

---

# 47. Conversation Behavior

A user may:

```text
click an option
write a custom answer
ask a follow-up question
add context
change their previous answer where allowed
```

These all occur inside the relevant task.

The agent's context should include:

```text
the task
its parent planning work
relevant specification
related decisions
its own conversation
```

Do not automatically inject unrelated task conversations.

---

# 48. Main Agent Observation Loop

Introduce or formalize a periodic/event-driven orchestration pass.

It should evaluate:

```text
new task created
user answered attention item
spec changed
implementation completed
verification failed
reconciliation found mismatch
configuration prerequisite changed
repository changed materially
```

The loop should decide:

```text
continue planning
create/update attention item
resolve item
update artifact
schedule agent investigation
prepare approval
```

It should not create noise just to appear active.

---

# 49. Avoid Polling the User

The agent should never generate board items like:

```text
Still waiting for your answer
Just checking in
Do you want me to continue?
```

The unresolved Needs Attention card is sufficient.

---

# 50. Avoid Duplicate Attention Items

Before creating an item, identify whether the issue already exists.

Deduplicate using stable semantic identity where possible.

Examples:

```text
same feature
same unresolved decision
same affected concern
```

Do not create CLR-104 because CLR-098 was ignored for an hour.

---

# 51. Resolve Cards Automatically When Appropriate

If repository evidence later resolves an Agent/Review issue safely:

```text
update the existing item
```

rather than creating another item.

Human decisions must still remain Human unless the user answers.

---

# 52. Suggested Board Columns

Target board:

```text
Planning
Needs Attention
Ready
In Progress
Done
```

### Planning

Packet is actively investigating or preparing work.

### Needs Attention

Packet cannot proceed with some portion of the work without user input.

### Ready

Planning is complete and work is ready for approval/start.

### In Progress

Implementation, verification, or reconciliation is running.

### Done

No current action remains.

Do not add separate columns for every internal lifecycle state.

---

# 53. Parent Tasks and Attention Cards

Prefer leaving the parent planning task in:

```text
Planning
```

while its user decisions appear in:

```text
Needs Attention
```

The parent may show:

```text
Waiting on 2 decisions
```

This avoids moving the same card back and forth and makes the outstanding decisions independently actionable.

---

# 54. New Task UX Implementation

Likely relevant files:

```text
src/ui/layout.rs
src/ui/planning_board.rs
src/ui/task_detail.rs
src/ui/task_chat.rs
src/app/root.rs
src/app/root/ui_actions.rs
src/core/planning_work.rs
```

Introduce:

```text
NewTaskDialog / NewTaskSheet
PlanningWorkKind
CreatePlanningWork command
```

Use typed commands.

Do not let the UI directly write state.

---

# 55. Remove Main Chat From Layout

Likely relevant files:

```text
src/ui/chat_pane.rs
src/ui/layout.rs
src/ui.rs
src/app/root.rs
src/persistence/chat_store.rs
src/core/turn/**
```

Implementation strategy:

1. remove Main Chat tab from normal rendering;
2. route new planning through work-item conversations;
3. leave existing chat persistence intact temporarily;
4. stop creating product dependencies on project-wide chat history;
5. retain developer diagnostics/history access if useful.

Do not delete storage until migration behavior is understood.

---

# 56. Change Planner Prompting Model

The planner prompt currently assumes a conversational interviewer.

Rewrite it around board orchestration.

The planner should be instructed:

```text
You are planning a work request.

Investigate first.

Record every actionable unresolved user decision as a structured board item.

Do not ask the user questions only in prose.

Resolve Agent-authority uncertainty yourself.

Create all currently actionable independent Human/Review decisions in the same planning pass.

Do not create dependent questions before their prerequisites resolve.

Continue all unblocked planning work.

Update the relevant specification as knowledge becomes settled.

Keep user-facing explanations understandable without technical expertise.
```

---

# 57. Remove Chat-Specific Planner Rules

Remove or rewrite policies like:

```text
ask one question in Main Chat
next_question_id controls the next chat question
one Human/Blocking question per reply
```

`next_question_id` may no longer be needed as a primary interaction mechanism.

Investigate whether it should:

```text
be removed
become an ordering hint
become an attention-focus hint
```

Do not retain it merely because old UI expected it.

---

# 58. Attention Priority

Still preserve priority.

Suggested model:

```text
Blocking
High
Normal
```

But priority should mean:

```text
impact on progress
```

not:

```text
how loudly the UI nags the user
```

---

# 59. Needs Attention Sorting

Sort roughly by:

```text
Blocking
then High
then Normal
```

Within priority:

```text
oldest actionable item first
```

Avoid model-generated arbitrary ordering.

---

# 60. Automatic Planning Continuation

After creating a new task, the system should automatically begin planning when Auto Plan is enabled.

Expected flow:

```text
New Task
↓
card appears immediately
↓
agent starts investigating
↓
card shows progress
↓
spec drafted
↓
attention items appear if needed
```

The user should not have to open a chat and say:

```text
start
```

---

# 61. User Response Continuation

After an attention item is answered:

```text
answer is persisted
parent task becomes runnable
planner resumes automatically
```

If Auto Plan is disabled:

```text
parent task becomes Ready to continue
```

and exposes one clear action.

---

# 62. Implementation Approval

Once planning is complete:

```text
Ready
```

should contain a clear approval action.

For nontechnical users the card should summarize:

```text
What Packet will build
Important decisions
Major ramifications
Known risks
What Packet recommends
```

Then:

```text
[ Approve & Build ]
[ Request changes ]
```

If Auto Build is enabled, approval may enqueue implementation.

Approval itself remains explicit.

---

# 63. Question-to-Task Conversion

A completed Question should offer contextual actions where appropriate.

Example:

```text
Packet answer:
The current cache is process-local.

[ Create task to change this ]
```

Selecting it creates a linked Feature or Bug task.

Do not require copying the question into New Task manually.

---

# 64. User-Friendly Language Requirement

Every Needs Attention item should undergo a readability check.

Avoid unexplained terms such as:

```text
idempotency
eventual consistency
ABI
CAS
RPO
write amplification
race condition
```

unless needed.

If a technical term matters:

```text
explain it immediately
```

Example:

```text
A race condition means two things can update the same data at the same
time and produce an unpredictable result.
```

---

# 65. Ramification Requirement

A Needs Attention item with options is invalid unless Packet explains consequences.

At minimum each meaningful option should provide:

```text
what changes
major benefit
major downside
how difficult it is to change later
```

The overall brief should explain:

```text
what happens if the user waits
```

---

# 66. Recommendation Requirement

If Packet has enough evidence to recommend an option, it should.

Do not use fake neutrality such as:

```text
Both options have pros and cons.
```

when one choice clearly matches the user's stated goals better.

Recommendation must include:

```text
recommended option
reason
confidence
important uncertainty
```

If evidence is insufficient:

```text
recommendation = none
```

and explain what is unknown.

---

# 67. Data Model Changes

Likely domains involved:

```text
src/domain/item.rs
src/domain/decision_brief.rs
src/core/planning_work.rs
src/core/workflow.rs
src/core/state.rs
```

Add only what is needed.

Potential additions:

```text
PlanningWorkKind
PlanningWorkStatus
BoardColumn
work_uid link on user interaction items
```

Prefer deriving UI state rather than persisting duplicate presentation fields.

---

# 68. Migration

Existing projects must continue loading.

Migration should:

1. version the planning-work ledger;
2. establish stable IDs for existing Work records;
3. infer an appropriate task type where possible;
4. preserve existing feature associations;
5. preserve existing conversations;
6. preserve open-item identities;
7. preserve task batches;
8. preserve approvals;
9. preserve implementation state.

Unknown old work should safely migrate as:

```text
PlanningWorkKind::Feature
```

only if evidence supports it.

Otherwise use an explicit legacy/other form rather than guessing.

---

# 69. Main Chat Migration

Existing Main Chat history should not become board items automatically.

That would create garbage.

Instead:

```text
historical chat remains archived/operator-local
current durable decisions remain in artifacts
new interaction uses work items
```

If an existing active planning conversation exists during migration, create at most one planning work item representing that current active request.

---

# 70. Artifact Rules

Continue enforcing:

```text
.kool-ade-packet/
```

as the sole shared Packet root.

Expected authority:

```text
.kool-ade-packet/planning/product/
    current product truth

.kool-ade-packet/planning/changes/
    planned change contracts

.kool-ade-packet/planning/decisions/
    material ADRs

.kool-ade-packet/planning/open-items.md
    unresolved board interactions

.kool-ade-packet/state/work.json
    planning work lifecycle

.kool-ade-packet/state/workflow.json
    workflow/process records

.kool-ade-packet/planning/tasks/
    implementation task stories

.kool-ade-packet/implementation/
    implementation and verification evidence
```

---

# 71. Recommended Implementation Phases

Implement in this order.

```text
P1  Lock Linux-only platform/provider contract
P2  Introduce typed planning work model
P3  Build the universal Needs Attention interaction model
P4  Add New Task flow
P5  Move planning interviews into task-centric orchestration
P6  Remove Main Chat from the primary UX
P7  Route approvals/blockers/configuration through Needs Attention
P8  Clean plan-comparison authority
P9  Tighten specs and ADR lifecycle
P10 Dogfood and simplify
```

---

# 72. P1 — Linux-Only Platform and Provider Contract

## Tasks

- declare Linux x86_64 current supported host;
- remove macOS/Windows claims;
- remove cross-platform CI expectations;
- simplify runtime capability model where possible;
- keep Bubblewrap detection;
- present missing Bubblewrap as actionable setup state;
- document current provider relay scope;
- expose unsupported provider as Needs Attention instead of opaque failure.

## Acceptance Criteria

- [ ] README says Linux-only.
- [ ] product scope says Linux-only.
- [ ] no product docs claim current Windows/macOS support.
- [ ] runtime code has no unnecessary fake cross-platform state machine.
- [ ] CI is intentionally Linux-only.
- [ ] missing Bubblewrap produces understandable setup guidance.
- [ ] unsupported provider produces understandable setup guidance.
- [ ] security behavior remains fail-closed.

---

# 73. P2 — Typed Planning Work

## Tasks

- version `state/work.json`;
- introduce stable work UID;
- introduce task type enum;
- replace numeric persisted columns;
- establish explicit status;
- add parent link support;
- migrate existing work records.

## Acceptance Criteria

- [ ] no workflow semantics depend on numeric board column.
- [ ] every planning request has stable identity.
- [ ] New Task kind survives restart.
- [ ] feature association survives rename.
- [ ] migrated work preserves current behavior.

---

# 74. P3 — Universal Needs Attention

## Tasks

- define board projection for Human/Review items;
- reuse DecisionBrief;
- require consequence information;
- render recommendations prominently;
- render option buttons;
- always support freeform response;
- show blocked parent work;
- expand technical evidence on demand.

## Acceptance Criteria

- [ ] every user-blocking item appears in Needs Attention.
- [ ] every option has understandable consequences.
- [ ] recommendation is visually obvious.
- [ ] recommendation rationale is visible.
- [ ] freeform response is always available.
- [ ] no user input requires opening Main Chat.
- [ ] Human selection remains explicit.

---

# 75. P4 — New Task Flow

## Tasks

Add:

```text
+ New Task
```

with:

```text
Feature
Bug
New Project
Question
```

Implement typed creation command.

Persist immediately.

Start planning asynchronously.

## Acceptance Criteria

- [ ] creating a task takes only type + description.
- [ ] task appears on board immediately.
- [ ] planning starts without another user message.
- [ ] task survives restart.
- [ ] user can add more context from its detail view.
- [ ] Question tasks can complete without creating specs.

---

# 76. P5 — Task-Centric Planning

## Tasks

Rewrite planner policy.

Remove one-question-per-chat-turn assumption.

Generate all currently actionable independent Human/Review items.

Resolve Agent items autonomously.

Continue unblocked work.

Maintain concise specifications.

## Acceptance Criteria

- [ ] planner does not ask unresolved questions only in prose.
- [ ] independent decisions appear simultaneously.
- [ ] dependent questions wait.
- [ ] parent planning continues where possible.
- [ ] answering an item resumes planning automatically.
- [ ] the user never needs to type `continue`.

---

# 77. P6 — Remove Main Chat From Primary UX

## Tasks

- remove Main Chat tab/pane from default layout;
- make board the central view;
- retain task conversations;
- preserve historical Main Chat storage;
- remove new code dependencies on Main Chat history;
- route generic questions through New Task → Question.

## Acceptance Criteria

- [ ] normal workflow contains no Main Chat.
- [ ] all planning can be initiated from New Task.
- [ ] all user responses can occur on cards/task details.
- [ ] old Main Chat history is not lost.
- [ ] no planning logic requires Main Chat to exist.

---

# 78. P7 — Route All User Action Through Board Items

Convert:

```text
feature approval
plan adoption
review approval
implementation blocker
independent-check blocker
configuration blocker
ownership issue
planning question
```

into the common Needs Attention experience.

Do not necessarily replace underlying application commands.

Reuse them.

## Acceptance Criteria

- [ ] actionable toasts are no longer the sole interaction.
- [ ] approvals have board representation.
- [ ] blockers have board representation.
- [ ] configuration problems have board representation.
- [ ] resolving the board item invokes typed application authority.

---

# 79. P8 — Clean Plan Comparison Authority

## Tasks

- make workflow record authoritative for comparison process;
- keep adopted plan in feature specification;
- migrate old comparison metadata;
- remove redundant `selected_alt` machine authority;
- simplify approval checks;
- preserve historical comparison evidence.

## Acceptance Criteria

- [ ] one machine authority exists for comparison lifecycle.
- [ ] adopted plan is readable in specification.
- [ ] approval fingerprint includes the adopted plan.
- [ ] restart does not require reconciling duplicate selected-plan values.
- [ ] old comparison records migrate safely.

---

# 80. P9 — Artifact Quality

Dogfood actual outputs.

Review:

```text
Feature task
Bug task
New Project task
Question task
Needs Attention decision
Approval
ADR
Reconciled product spec
```

Ensure each artifact is concise.

Explicitly reject artifacts that merely repeat information available elsewhere.

---

# 81. P10 — Dogfood Scenarios

## Scenario A — Simple Feature

User:

```text
Add the ability to export planning tasks as Markdown.
```

Expected:

```text
New Task
Planning card
repository investigation
Draft feature spec
0-N attention cards
approval card
task generation
```

No Main Chat.

---

## Scenario B — Ambiguous Feature

User:

```text
Add user accounts.
```

Packet should independently investigate and likely create several parallel attention items such as:

```text
Who needs accounts?
Where should identity live?
Should accounts sync between devices?
```

But only if those questions cannot be safely inferred.

Each card must include recommendations.

---

## Scenario C — Bug

User:

```text
The task detail modal cuts off long blocker messages.
```

Packet should investigate first.

Ideally:

```text
no questions
concise change spec
recommended fix
ready for approval
```

---

## Scenario D — Question

User:

```text
Why does Packet use Bubblewrap?
```

Packet creates a Question card.

It investigates.

It posts an understandable answer.

It marks the task Done.

No product spec change.

---

## Scenario E — Question Becomes Work

User:

```text
Why can't Packet use hosted Anthropic through Pi?
```

Packet answers.

Then offers:

```text
[ Create task to add hosted-provider support ]
```

Selecting it creates a linked Feature task.

---

## Scenario F — Multiple Independent Decisions

Feature requires:

```text
storage choice
notification choice
theme choice
```

All three are independently actionable.

All three should appear in Needs Attention without making the user answer them sequentially.

---

## Scenario G — Dependent Decision

Authentication approach depends on storage choice.

Do not surface authentication implementation decision until storage has been resolved.

---

## Scenario H — No User Action

Agent finds an implementation detail it can resolve safely.

It resolves it.

No Needs Attention item is created.

---

## Scenario I — Unsupported Provider

Configure a provider the current relay cannot support.

Expected:

```text
Needs Attention

Packet cannot safely connect to this model provider.

Recommendation:
Use your configured local OpenAI-compatible provider.

Ramification:
Repository-aware planning is paused until this is fixed.

[ Open settings ]
[ Retry ]
```

---

## Scenario J — Nontechnical Reader

Give a person unfamiliar with the codebase a Needs Attention decision.

After reading only the card they should be able to explain:

```text
what Packet needs
why
Packet's recommendation
what happens if they accept
what happens if they choose another option
whether they can change it later
what happens if they wait
```

If they cannot, the interaction fails.

---

# 82. Required Regression Tests

## Planning Work

- [ ] stable work identity survives rename
- [ ] work type survives restart
- [ ] status survives restart
- [ ] migration from legacy work.json
- [ ] parent feature relation survives rename

## Board

- [ ] typed status maps to correct board column
- [ ] Needs Attention only contains actionable user items
- [ ] resolved item leaves Needs Attention
- [ ] independent decisions render simultaneously
- [ ] dependent decision remains hidden until eligible

## New Task

- [ ] Feature creation
- [ ] Bug creation
- [ ] New Project creation
- [ ] Question creation
- [ ] description persists
- [ ] planner starts automatically

## Attention UX

- [ ] recommendation rendered
- [ ] confidence rendered
- [ ] option consequences rendered
- [ ] defer consequence rendered
- [ ] freeform always available
- [ ] option click records Human response
- [ ] freeform answer records Human response
- [ ] response resumes parent planning

## Agent Behavior

- [ ] Agent authority issue resolved without user card
- [ ] Human issue creates card
- [ ] Review issue creates card
- [ ] no prose-only unresolved user question
- [ ] no duplicate attention item
- [ ] planning continues around unrelated blocker

## Main Chat Removal

- [ ] normal UI contains no Main Chat
- [ ] old history still loads safely
- [ ] task chat works
- [ ] Question task replaces generic question flow
- [ ] no workflow assumes active Main Chat

## Platform

- [ ] Linux + Bubblewrap works
- [ ] missing Bubblewrap fails clearly
- [ ] provider relay works for supported local provider
- [ ] unsupported provider fails safely
- [ ] docs contain no current macOS/Windows support claim

## Plan Comparison

- [ ] workflow comparison survives restart
- [ ] adopted plan appears in feature spec
- [ ] approval contract includes selected plan
- [ ] no duplicate selected-plan authority is required
- [ ] legacy comparison metadata migrates

---

# 83. Quality Gates

Run at completion:

```bash
cargo +1.98.1 fmt --all --check
cargo +1.98.1 check --locked --all-targets
cargo +1.98.1 test --locked --all-targets -- --test-threads=1
cargo +1.98.1 clippy --locked --all-targets -- -D warnings
git diff --check
```

CI may remain Linux-only.

---

# 84. Implementation Rules

## Do not rewrite Packet

This is an evolution of the current hardened architecture.

Preserve:

```text
ArtifactLayout
stable artifact identities
ChangeStatus
writer gates
artifact transactions
repository locks
sandbox boundaries
typed ApplicationCommand
DecisionBrief
task conversations
implementation queue
reconciliation lifecycle
```

---

## Do not create another giant abstraction hierarchy

The important new abstractions are limited to:

```text
typed planning work
typed board columns/status
Needs Attention view projection
New Task intent
work-item parent linkage
```

Everything else should reuse existing systems.

---

## Do not move application authority into the model

The agent may decide:

```text
what issue exists
what alternatives exist
what it recommends
why it recommends it
what ramifications matter
what specification should say
```

Rust decides:

```text
whether an action is allowed
whether a user answer is valid
whether approval exists
whether a task may start
whether publication is allowed
whether a path may be written
whether a workflow transition is legal
```

---

# 85. Completion Definition

This effort is complete when a user can operate Packet primarily through the board.

A typical experience should be:

```text
User clicks New Task.

User says:
"Add offline support."

Packet creates a Planning card.

Packet investigates.

Packet determines two independent product decisions require the user.

Two clear Needs Attention cards appear.

Each explains:
- the issue
- Packet's recommendation
- why
- ramifications
- options
- what happens if deferred

The user answers one now and one later.

Packet continues all work that is not blocked.

The second answer eventually arrives.

Packet completes the change specification.

A Ready card explains what will be built.

The user approves it.

Implementation runs.

Any meaningful blocker appears in Needs Attention.

After verification, Packet reconciles the product specification.

The task becomes Done.
```

At no point should the user need to wonder:

```text
Where do I answer this?
What is Packet waiting on?
What does Packet recommend?
Why does this matter?
What happens if I choose this?
What conversation was this question buried in?
Do I need to tell Packet to continue?
```

That is the target experience.

---

# 86. Final Product Model

The desired architecture should converge toward:

```text
                       User
                        │
                  ┌─────▼─────┐
                  │ New Task  │
                  └─────┬─────┘
                        │
                        ▼
                Planning Work Item
                        │
           ┌────────────┼─────────────┐
           │            │             │
           ▼            ▼             ▼
       Agent Work    Specification   Needs Attention
           │            │             │
           │            │      ┌──────▼──────┐
           │            │      │ User Choice │
           │            │      └──────┬──────┘
           │            │             │
           └────────────┴─────────────┘
                        │
                        ▼
                       Ready
                        │
                   User Approval
                        │
                        ▼
                  Implementation
                        │
                        ▼
                   Verification
                        │
                        ▼
                  Reconciliation
                        │
                        ▼
                       Done
```

The main agent operates across this system but is not itself the UI.

The board is the UI.

The specification is the shared truth.

Needs Attention is the contract between Packet and the user.
```