# MVP Plan: Git-Native LLM Specification Planner

## 1. Goal

Build a simple desktop application that helps a team create a software specification through an LLM-led interview.

The application should focus on one primary workflow:

> The user talks with the planning agent, the agent builds the specification, and unresolved questions automatically become a prioritized queue for the appropriate stakeholders.

The MVP should avoid project-management features, complex workflow systems, dashboards, and implementation planning.

---

# 2. Core Principles

## Specification First

The specification is the primary artifact and the largest element in the application.

Users do not directly edit the specification.

The specification is:

* generated from the conversation
* updated by the planning agent
* stored as Markdown
* rendered as formatted content in the UI
* read-only to users

This keeps the specification synchronized with the reasoning and decisions made during planning.

---

## Chat Drives the Specification

All changes to the specification happen through conversation.

Examples:

> We need users to authenticate with SSO.

> The service needs to support 10,000 concurrent users.

> Actually, remove the requirement for local accounts.

The agent interprets the conversation and updates the specification accordingly.

---

## Git Is the Storage System

The project connects to an existing Git repository.

All durable planning artifacts are stored inside that repository.

The application should not introduce a separate database for project state.

Git provides:

* storage
* history
* diffs
* rollback
* collaboration history
* branching

Temporary runtime state may exist in memory.

---

## External AI Harnesses Own LLM Interaction

The application must not call LLM provider APIs directly.

All LLM interactions must go through an external AI harness.

For the MVP, the initial supported harness is:

```text
pi CLI
```

The application is responsible for:

* preparing context
* invoking the harness
* supplying instructions
* receiving the harness response
* validating structured output
* applying resulting changes

The harness is responsible for:

* model selection
* provider authentication
* model invocation
* tool calling
* model-specific configuration
* inference behavior

This boundary should remain clean so additional harnesses can be supported later without redesigning the planner.

Potential future harnesses may include:

* Codex CLI
* GitHub Copilot CLI
* Claude Code
* OpenCode
* other agent or model harnesses

The MVP only needs Pi.

---

# 3. Desktop Application

The MVP is a desktop application implemented as a pure Rust solution.

The application should contain three major areas:

```text
┌──────────────────────────────────────────────────────────────────────┐
│ Project                                                      Git    │
├─────────────────────┬──────────────────────────────┬─────────────────┤
│                     │                              │                 │
│ CHAT                │       SPECIFICATION          │ OPEN ITEMS      │
│                     │                              │                 │
│ Agent interview     │ Read-only rendered Markdown │ 🔴 Blocking     │
│                     │                              │ Security        │
│ User responses      │                              │                 │
│                     │                              │ 🟡 High         │
│ Agent interviews    │                              │ Product         │
│                     │                              │                 │
│ ⚪ Unassigned   │    │                              │ Ownership       │
│                     │                              │                 │
├─────────────────────┴──────────────────────────────┴─────────────────┤
│                                                                      │
└──────────────────────────────────────────────────────────────────────┘
```

The specification should receive the most screen space.

---

# 4. Primary Artifacts

Keep the MVP artifact model small.

## Specification

```text
planning/specification.md
```

The complete current specification.

---

## Open Items

```text
planning/open-items.md
```

Contains unresolved:

* questions
* ambiguities
* assumptions requiring validation
* missing decisions
* missing ownership

The application may internally represent these items as structured Rust types while serializing them to Markdown.

---

## Configuration

```text
.planner/config.md
```

Defines basic stakeholder and ownership information.

Example:

```markdown
# Stakeholders

## Product
- Zach
- Sarah

## Development
- Alex
- Chris

## QA
- Taylor

## InfoSec
- Morgan
```

The exact format can evolve later.

The important MVP requirement is simply being able to map a category to a user or group.

---

# 5. Open Item Model

Every unresolved planning issue should contain only the information needed for the workflow.

Example:

```markdown
## CLR-012

**Priority:** Blocking  
**Type:** Ambiguity  
**Category:** Security  
**Assigned To:** InfoSec  

### Question

Should API tokens expire automatically?

### Reason

The specification defines token creation but does not define token lifetime.
```

An item should contain:

* ID
* priority
* type
* category/tag
* assigned user or group
* question
* reason/context
* status

Do not build a complicated workflow around these in the MVP.

An item is either:

* Open
* Resolved

---

# 6. Item Types

Start with four types.

## Question

Information the agent cannot determine.

## Ambiguity

Something in the current specification could reasonably mean multiple things.

## Assumption

Something the agent currently assumes but believes should be confirmed.

## Ownership

The system knows a question requires a particular category of stakeholder but cannot determine who owns that category.

Do not introduce separate risk, decision, dependency, or issue systems yet.

Those can be added after seeing how teams actually use the product.

---

# 7. Categorization

The agent assigns each open item a category.

Initial categories can include:

* General
* Product
* Development
* QA
* InfoSec
* UX
* Operations

Projects can add additional categories later.

The category is used primarily for routing.

---

# 8. Ownership

Each category may optionally map to:

* a person
* a group

Example:

```text
Product → Product Team
QA → QA Team
InfoSec → Morgan
```

If a category has no configured owner, the planner automatically creates an Ownership item.

Example:

```text
Category "Data Governance" has no assigned stakeholder.
```

That item appears in the right-hand panel.

This prevents questions from silently becoming orphaned.

---

# 9. Priority

The planning agent determines priority.

Keep the priority model simple:

## Blocking

The specification cannot meaningfully progress without an answer.

## High

The answer could significantly change the specification or architecture.

## Normal

The answer matters but does not currently prevent progress.

The agent should consider:

* how much of the specification depends on the answer
* whether different answers could substantially change the solution
* whether later questions depend on it
* whether incorrect assumptions would be expensive to reverse

Open items are sorted:

```text
Blocking
↓
High
↓
Normal
```

Within the same priority, the agent determines which question is most useful to resolve next.

---

# 10. New Project Workflow

## Step 1: Connect Repository

The user selects or connects a Git repository.

The planner inspects enough of the repository to understand the project context.

For the MVP this should focus on:

* README
* documentation
* major directory structure
* project configuration
* obvious architecture files
* existing specification/planning artifacts

Deep repository analysis can come later.

---

## Step 2: Identify User

The application knows:

* the current user
* groups they belong to

Example:

```text
User: Zach

Groups:
- Development
- Architecture
```

---

## Step 3: Start Interview

For a new specification, the agent initiates the conversation.

Example:

> What are we trying to build or change?

The user explains the idea naturally.

---

## Step 4: Send Planning Turn Through Pi

The desktop application constructs a planning request containing only the context needed for the current turn.

This may include:

* user message
* current specification
* current open items
* stakeholder configuration
* relevant repository context
* relevant imported documents
* relevant MCP results
* planner instructions

The application then invokes the Pi CLI.

Conceptually:

```text
Planner App
    ↓
Build context
    ↓
Invoke Pi CLI
    ↓
Pi executes model interaction
    ↓
Structured response
    ↓
Planner validates response
```

The desktop application must never require direct knowledge of the underlying model provider.

---

## Step 5: Build Initial Specification

The agent extracts what it can from the conversation and creates:

```text
planning/specification.md
```

The formatted specification appears in the center panel.

---

## Step 6: Generate Open Items

The agent reviews the current specification and identifies:

* missing information
* ambiguity
* assumptions
* unresolved questions

These automatically populate the right-hand panel.

---

## Step 7: Categorize and Assign

Each item receives:

* type
* category
* priority
* owner

If ownership cannot be determined, an Ownership item is created.

---

## Step 8: Select Next Question

The agent examines the queue.

It should only ask questions that are:

* assigned directly to the current user
* assigned to one of the current user's groups
* categorized as General

The agent selects the highest-priority eligible item.

---

## Step 9: Ask Through Chat

The agent asks the selected question naturally.

Example:

> For CLR-012, should API tokens expire automatically, or should they remain valid until explicitly revoked?

The user responds in chat.

---

## Step 10: Update Specification

The next user response is sent through Pi along with the relevant planning context.

The agent uses the answer to:

1. update the specification
2. resolve the related open item
3. reevaluate affected portions of the specification
4. create any newly discovered open items
5. reprioritize the queue
6. select the next eligible question

---

## Step 11: Continue Interview

The loop continues:

```text
Ask
↓
Answer
↓
Invoke Pi
↓
Update Spec
↓
Resolve Item
↓
Analyze
↓
Create / Update Queue
↓
Ask Next Eligible Item
```

---

# 11. User-Specific Question Routing

This is a critical behavior.

Suppose the queue contains:

```text
1. BLOCKING | InfoSec     | Morgan
2. BLOCKING | Product     | Product
3. HIGH     | Development | Zach
4. HIGH     | General     | Everyone
```

If Zach belongs to Development, the agent should not ask questions 1 or 2.

It should ask:

```text
3. HIGH | Development | Zach
```

If that is resolved, it can ask the General question.

Questions belonging to other stakeholders remain visible in the right-hand panel but are not included in the current user's interview.

---

# 12. Right-Hand Panel

The right-hand panel should be intentionally simple.

Example:

```text
OPEN ITEMS

🔴 BLOCKING

Security
CLR-012
Should tokens expire?
Assigned: InfoSec

Product
CLR-014
What happens when...?
Assigned: Product

⚠ OWNERSHIP

Data Governance
No owner configured

🟡 HIGH

Development
CLR-021
Should retries...?
Assigned: Zach
```

Users should be able to:

* see the queue
* see priority
* see category
* see assignment
* select an item to inspect its context

The interview agent remains responsible for moving through the queue.

---

# 13. Specification Rendering

Specifications and supporting artifacts should always be stored as Markdown.

The desktop application renders Markdown as formatted content.

Users see:

* headings
* lists
* tables
* code blocks
* links
* emphasis

instead of raw Markdown.

The underlying file remains plain Markdown in Git.

---

# 14. Pi Integration

Pi is the only AI harness required for the MVP.

The planner should invoke Pi as an external process rather than embedding its functionality.

Conceptually:

```text
Rust Desktop App
      ↓
Pi Adapter
      ↓
pi CLI
      ↓
Configured LLM
```

The Pi adapter should be responsible for:

* locating the Pi executable
* checking that Pi is available
* launching Pi
* providing the project working directory
* providing planner instructions
* passing relevant context
* capturing stdout/stderr
* detecting execution failure
* parsing the response
* returning a normalized result to the planner

The rest of the application should not depend directly on Pi-specific behavior.

---

# 15. Harness Abstraction

Even though Pi is the only MVP implementation, define a narrow internal abstraction.

Conceptually:

```rust
trait AiHarness {
    fn execute(&self, request: PlanningRequest) -> Result<PlanningResponse>;
}
```

Pi implements that abstraction:

```text
AiHarness
    │
    └── PiHarness
```

Future implementations might include:

```text
AiHarness
    ├── PiHarness
    ├── CodexHarness
    ├── CopilotHarness
    └── ClaudeCodeHarness
```

Do not build those additional implementations in the MVP.

The abstraction exists only to prevent Pi from leaking throughout the codebase.

---

# 16. Structured Agent Response

The planner should not attempt to infer application state from arbitrary prose returned by Pi.

Each planning turn should produce a predictable structured result containing approximately:

```text
assistant_message
updated_specification
open_items_added
open_items_updated
open_items_resolved
next_question
```

The exact transport format can be JSON or another machine-readable format supported reliably by the harness.

The Rust application validates this result before changing files.

If the result is invalid:

* do not mutate project files
* report the harness failure
* preserve the previous valid planning state

---

# 17. Separation of Responsibilities

## Rust Desktop Application

Owns:

* UI
* project selection
* repository access
* Git operations
* Markdown rendering
* open-item state
* stakeholder configuration
* document import
* MCP configuration
* context gathering
* Pi process execution
* output validation
* application of valid changes

## Pi

Owns:

* LLM interaction
* reasoning
* model/tool execution
* interpretation of planning context
* specification generation
* ambiguity detection
* prioritization recommendations
* question generation

## Git

Owns:

* durable state
* version history
* rollback

This separation should remain clear.

---

# 18. Repository Access by Pi

Pi should run with the connected project repository as its working directory.

This allows Pi to inspect the codebase using its own available capabilities.

The planner should not duplicate every repository-understanding feature itself.

The planner may still provide targeted context when useful, but Pi should be allowed to inspect the repository directly.

This is particularly important for questions such as:

> How does authentication work today?

> Does this project already have a retry abstraction?

> What tests currently cover this behavior?

The agent should investigate before creating a human clarification.

---

# 19. MCP

MCP should also be exposed through the external AI harness where practical.

The preferred architecture is:

```text
Planner
   ↓
Pi
   ↓
Configured MCP Servers
```

rather than:

```text
Planner
   ↓
Custom MCP orchestration
   ↓
LLM
```

The planner should avoid becoming another general-purpose agent runtime.

For the MVP, the application needs to provide a way to configure which MCP servers are available to the planning session.

Pi remains responsible for using them during model execution.

---

# 20. Git Behavior

Keep Git behavior minimal.

The application should:

* read from the repository
* create planning files
* update planning files
* commit meaningful changes

Do not commit every message.

A reasonable checkpoint is after a meaningful planning update.

Example:

```text
planner: establish initial specification

planner: resolve authentication requirements

planner: clarify deployment constraints
```

Git history should remain readable.

---

# 21. Document Import

The MVP should allow a user to import documents.

Imported files are copied directly into the repository.

Example:

```text
planning/imports/security-requirements.pdf
planning/imports/product-notes.docx
```

Where useful, an LLM-friendly Markdown extraction may also be stored:

```text
planning/imports/security-requirements.md
```

Because the imported artifacts live in the repository, Pi can inspect them as part of normal project context.

---

# 22. Agent Behavior

The planning agent should follow a simple loop.

```text
Read current specification
        ↓
Read unresolved items
        ↓
Review new conversation
        ↓
Inspect repository / MCP if needed
        ↓
Update specification
        ↓
Detect missing information
        ↓
Resolve anything discoverable
        ↓
Update open-item queue
        ↓
Assign categories / owners
        ↓
Prioritize items
        ↓
Find highest-priority item eligible for current user
        ↓
Ask question
```

The agent should favor progress over exhaustive analysis.

The goal is not to identify every possible question immediately.

The goal is to continuously discover the next most important missing information.

---

# 23. MVP Architecture

Keep the architecture small.

```text
┌─────────────────────────────────┐
│        Rust Desktop App         │
│                                 │
│ Chat                            │
│ Specification Renderer          │
│ Open Item Panel                 │
│                                 │
├─────────────────────────────────┤
│ Planner Core                    │
│                                 │
│ Project State                   │
│ Routing                         │
│ Git Operations                  │
│ Document Import                 │
│ Harness Adapter                 │
├─────────────────────────────────┤
│          Pi Harness             │
│              │                  │
│           pi CLI                │
├──────────────┼──────────────────┤
│              │                  │
│         MCP Servers             │
│              │                  │
│       Configured LLM            │
├──────────────┼──────────────────┤
│              │                  │
│              Git                │
└─────────────────────────────────┘
```

Avoid splitting this into multiple services for the MVP.

It is a desktop application with an external AI runtime.

---

# 24. Explicit MVP Non-Goals

Do not build these yet:

* direct OpenAI API integration
* direct Anthropic API integration
* custom model-provider abstraction
* embedded inference
* multiple AI harness implementations
* Jira integration
* readiness percentages
* traceability graphs
* complex review workflows
* multiple specialized agents
* risk management system
* ADR generation
* organizational dashboards
* analytics
* custom approval workflows
* complicated permissions
* specification editing
* separate project database
* project management features
* autonomous coding

The planner coordinates planning.

Pi provides the intelligence.

---

# 25. MVP Success Test

The MVP succeeds if this experience works well:

```text
User connects repository
        ↓
Planner launches Pi
        ↓
Agent asks:
"What are we trying to build?"
        ↓
User describes feature
        ↓
Pi analyzes request + repository
        ↓
Specification appears
        ↓
Agent detects important missing information
        ↓
Open items appear automatically
        ↓
Items are categorized and assigned
        ↓
Agent asks the current user the highest-priority relevant question
        ↓
User answers
        ↓
Response is sent through Pi
        ↓
Specification updates
        ↓
Question disappears from open queue
        ↓
New questions may appear
        ↓
Interview continues
```

The central product experiment is:

> Can an external AI coding/agent harness continuously turn a loosely defined project idea into a better specification by identifying, routing, and resolving the most important unanswered questions?

Everything in the MVP should serve that experiment.


# Product intent and task generation

The interview must establish the product or feature's problem, goal, intended users,
desired outcome, scope, exclusions, constraints, and observable success criteria.
The agent records this understanding in the specification and asks focused questions
about missing intent before assuming an implementation direction.

Once the agent is satisfied and blocking questions are resolved, it summarizes the
agreed intent and asks whether to proceed to task generation. Readiness is stored in
`.planner/workflow.json` against the exact reviewed specification. The application
requires explicit user consent through Generate task stories or an affirmative reply
to the pending offer. Further discussion invalidates the previous readiness decision.

Task generation uses an ordered outline and a separate model response per detailed
story. Completed dependency stories are included as context for subsequent tasks.
Incomplete or invalid outputs receive validation feedback and up to three attempts
per stage under the same planning-turn deadline. Completed stories are checkpointed
in private project state and reused when retrying an unchanged plan and tracked
checkout. Attempt evidence is retained for diagnosis. The app pins titles, purposes,
and reference mappings to the accepted outline; wording drift alone is not fatal.
Generation may not change the
approved specification or unresolved-item queue. Missing details, incomplete scope
coverage, invalid dependencies, cancellation, or changed source artifacts prevent
completion of the task batch. Individually validated stories remain saved and visible.

Each story leads with its own problem and rationale, plus the observable outcome
that ticket alone delivers. These must not repeat the overall product mission.
Each story also contains implementation context,
technical contracts, verification commands and expected evidence, a user story,
purpose, specification references,
dependencies, affected files/components, concrete implementation steps, observable
acceptance criteria, explicit test setup/actions/assertions, edge/failure cases,
constraints, exclusions, rollout/compatibility notes, and a definition of done.
Dependencies must refer to earlier numbered stories; every approved scope item and
success criterion must be covered across the task set.

The application saves each individually validated story under `planning/tasks/<feature>/`,
with files such as `001-<descriptive-task-title>.md`, a README index, and the approved
specification snapshot. The README and task panel show in-progress counts while
stories are generated; only final batch validation marks the batch complete. Saved
stories survive failure, cancellation, and restart, and resume preserves their contents.
Existing batches are preserved; subsequent batches use a
new numbered feature directory. The files and workflow metadata are included in a
git checkpoint, and the latest batch can be read in the Task stories tab. Producing
task files does not authorize executing them.


## Ticket implementation and pull requests

Each numbered story offers an Implement action. Selecting it authorizes Pi to
implement that ticket in a dedicated worktree and Packet to publish the verified
result as a GitHub pull request. A stable branch and persisted worktree identity
allow Resume implementation to review existing changes and continue after failure,
cancellation, or restart. Original-checkout drafts are not copied or modified.
Repository AGENTS.md instructions apply during implementation.

The application records execution status and evidence outside tracked planning
artifacts, prevents concurrent execution of the same ticket, and refuses changed
ticket scope or mismatched worktrees. Pi must report evidence for every listed
acceptance criterion and no remaining work. Packet reruns verification commands,
checks the diff, commits the result, pushes to origin, and creates or reuses a PR
against the original starting branch. Failed checks never create a PR. A publishing
retry can reuse an unchanged verified commit. No automatic merging occurs.


New implementations fetch the connected branch from origin and start from the
latest commit compatible with a fast-forward. Local commits ahead of origin are
preserved; divergent history and fetch failures stop execution before Pi starts.
The original checkout is unchanged. Resume retains the existing base and worktree.

Task stories appear on a Kanban board with To do, In progress, In review, Needs
attention, and Done columns. Cards select the story detail and its available action.
Background PR checks run on connection and every minute while connected. GitHub's
confirmed OPEN, CLOSED, and MERGED states drive review, attention, and completion;
closed PRs may return to review if reopened. Polling uses the per-ticket execution
lock, persists status outside tracked planning artifacts, and preserves the last
confirmed state on errors with visible stale-state feedback. PRs from older task
batches are also refreshed. Published tickets expose their PR rather than allowing
a duplicate implementation. Confirmed merged PRs no longer require polling.


Implementation report parsing, acceptance-evidence validation, verification-command
failures, and diff-check failures trigger up to three automatic corrections after
the initial attempt. Each correction receives the precise error and preceding
response, preserves the same worktree, and uses only the remaining original turn
budget. Attempt responses, verification results, and correction reasons remain
immutable artifacts. Explicit blockers, cancellation, expired budgets, changed
worktree identity, and exhausted corrections stop without publishing. Manual resume
includes the saved stop reason. Transport and publication errors do not restart
implementation automatically.
