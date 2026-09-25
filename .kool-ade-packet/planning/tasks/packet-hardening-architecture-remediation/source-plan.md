# Packet Hardening and Architecture Remediation Plan

## 1. Purpose

Harden Packet into a reliable, maintainable planning and implementation system suitable for nontechnical and lightly technical users, especially vibe-coders.

Packet should help a user:

- understand what is being built;
- understand decisions before making them;
- understand the consequences, risks, costs, and reversibility of those decisions;
- receive a clear recommendation when a meaningful choice exists;
- maintain concise, accurate, durable specifications;
- maintain concise ADRs that explain material architectural and product decisions;
- convert approved intent into implementation-ready work;
- safely execute implementation without granting an LLM unnecessary authority over the user's machine or Git repository;
- reconcile completed implementation back into current product truth;
- retain all shared Packet-owned artifacts under `.kool-ade-packet/`.

This remediation is primarily an architecture, safety, artifact, and maintainability effort. Avoid adding unrelated product features while performing it.

---

# 2. Target Architecture

Packet should converge on the following high-level lifecycle:

```text
Conversation
    |
    v
Planning Engine
    |
    +--> Evidence / Context Retrieval
    |
    +--> Decisions and Recommendations
    |
    +--> Open Planning Work
    |
    v
.kool-ade-packet/
    |
    +--> Current Product Specification
    |
    +--> Feature / Change Specifications
    |
    +--> Decisions / ADRs
    |
    +--> Open and Resolved Planning Items
    |
    +--> Task Plans
    |
    v
Explicit Human Approval
    |
    v
Task Generation
    |
    v
Sandboxed Implementation
    |
    v
Independent Verification
    |
    v
Controlled Publication
    |
    v
Reconciliation
    |
    v
Updated Product Truth
```

The primary architectural principle is:

> Rust owns safety, permissions, invariants, persistence, state transitions, and execution boundaries. The LLM owns interpretation, relevance, recommendations, tradeoffs, task decomposition, and appropriate planning detail.

Do not encode subjective planning judgment in deterministic application code unless a safety or persistence invariant requires it.

---

# 3. Implementation Rules

These rules apply to every remediation phase.

## 3.1 Preserve behavior unless the phase explicitly changes it

Do not opportunistically redesign unrelated features.

## 3.2 Use atomic commits

Each meaningful remediation step should produce a focused commit that can be reviewed or reverted independently.

Avoid combining artifact migration, UI changes, Git hardening, task-model changes, and major module decomposition in one commit.

## 3.3 Update tests with behavior

Every altered invariant must receive corresponding tests in the same change.

## 3.4 Do not weaken existing safety checks during refactoring

Existing path containment, writer gates, transaction recovery, approval hashes, Git ancestry verification, worktree isolation, and routing validation must remain operative unless replaced by a demonstrably stronger mechanism.

## 3.5 Prefer deletion over compatibility layering

When a migration is complete and proven, remove the legacy path or compatibility branch.

Do not leave permanent logic such as:

```text
if legacy path exists:
    use legacy path
else:
    use new path
```

Migration compatibility belongs in migration code, not normal runtime behavior.

## 3.6 Avoid new abstractions without a concrete ownership boundary

Refactor around responsibilities and state machines.

Do not introduce factories, repositories, managers, providers, services, or generic interfaces merely to make files smaller.

## 3.7 No model-authored filesystem paths for Packet artifacts

The application owns artifact placement.

The model refers to logical IDs.

## 3.8 No model-authored Git publication operations

Packet owns commit, integration, push, PR creation, merge, and publication.

## 3.9 No model-authored shell should execute with unrestricted host authority

Verification and implementation command execution must ultimately be contained by a defined execution boundary.

---

# 4. Phase 0: Establish a Green Baseline

## Goal

Create a trustworthy baseline before architectural changes begin.

## Work

1. Run the complete test suite.
2. Run `cargo fmt --check`.
3. Run `cargo clippy --all-targets`.
4. Record existing warnings and failures.
5. Run any current integration tests involving:
   - task generation;
   - multi-repository work;
   - implementation;
   - reconciliation;
   - Git publication;
   - task recovery;
   - artifact migration.
6. Record the current artifact tree generated for:
   - a fresh repository;
   - the Packet repository itself;
   - a repository containing legacy `planning/`;
   - a repository containing legacy `.planner/`;
   - a repository containing existing task batches.

Create a short remediation baseline document at:

```text
.kool-ade-packet/planning/archive/remediation-baseline.md
```

The document should describe only the baseline facts required to validate the migration later.

Do not turn it into a test transcript dump.

## Acceptance Criteria

- Existing tests have a recorded pass/fail baseline.
- Existing clippy warnings are known.
- Current artifact locations are enumerated.
- No production behavior has changed.
- The baseline itself lives under `.kool-ade-packet/`.

---

# 5. Phase 1: Introduce a Single Artifact Layout Authority

## Goal

Make one module authoritative for every Packet-owned repository artifact.

No other subsystem should construct Packet artifact paths manually.

## Target Layout

```text
.kool-ade-packet/
  manifest.json

  config/
    project.json
    mcp.json

  planning/
    product/
      index.md
      ...
    changes/
      CHG-*/
        specification.md
    decisions/
      ADR-*.md
    open-items.md
    resolved-items.json
    imports/
    tasks/
    archive/

  state/
    workflow.json
    work.json

  implementation/
    ...
```

Private operator-level information remains under `$PACKET_HOME`.

## Work

Create a dedicated artifact-layout module, for example:

```text
src/artifacts/layout.rs
```

It should expose typed or strongly named helpers for:

- Packet root;
- manifest;
- product root;
- product index;
- product modules;
- change specifications;
- decisions;
- open items;
- resolved items;
- imports;
- tasks;
- workflow state;
- work state;
- implementation state;
- archive.

Replace path constants currently scattered through:

- `src/artifacts/artifacts.rs`;
- `src/artifacts/packet.rs`;
- `src/artifacts/product_docs.rs`;
- `src/artifacts/task_docs.rs`;
- `src/core/state.rs`;
- `src/core/apply.rs`;
- `src/core/workflow.rs`;
- `src/core/context_build.rs`;
- `src/core/implementation.rs`;
- `src/core/implementation_queue.rs`;
- `src/core/reconciliation.rs`;
- related application code and tests.

Do not move files yet during this phase unless necessary to establish tests.

## Acceptance Criteria

- Every Packet-owned project artifact has a canonical helper.
- Runtime code no longer manually joins strings such as `planning/...`, `.planner/...`, or `adr/...`.
- Path ownership is centralized.
- Existing behavior remains compatible before migration.
- Tests prove canonical paths cannot escape the repository.

---

# 6. Phase 2: Complete the `.kool-ade-packet` Migration

## Goal

Make `.kool-ade-packet/` the sole location for all shared Packet project artifacts.

## Legacy Sources to Migrate

At minimum:

```text
planning/product/
planning/features/
planning/open-items.md
planning/resolved-items.json
planning/imports/
planning/tasks/
.planner/config.md
.planner/mcp.json
.planner/workflow.json
adr/
SPECIFICATION.md
planning/specification.md
planning/archive/
```

## Migration Rules

Introduce an artifact schema version in:

```text
.kool-ade-packet/manifest.json
```

Example conceptually:

```json
{
  "schemaVersion": 2,
  "product": "Packet"
}
```

Migration runs during connection before normal planning state is loaded.

The migration must:

1. acquire the Packet writer lock;
2. inspect the existing layout;
3. determine the migration source version;
4. refuse destructive ambiguity;
5. build the entire migration plan before mutating;
6. preserve bytes for historical artifacts;
7. move current artifacts to canonical destinations;
8. write the manifest last;
9. checkpoint migrated tracked artifacts;
10. be safe to restart after interruption.

If both a legacy file and target file exist with different content, do not pick one silently.

Surface an actionable migration conflict.

If content is identical, deduplicate safely.

## Required New Locations

Examples:

```text
planning/product/
    -> .kool-ade-packet/planning/product/

planning/features/
    -> .kool-ade-packet/planning/changes/

planning/open-items.md
    -> .kool-ade-packet/planning/open-items.md

planning/resolved-items.json
    -> .kool-ade-packet/planning/resolved-items.json

planning/imports/
    -> .kool-ade-packet/planning/imports/

planning/tasks/
    -> .kool-ade-packet/planning/tasks/

.planner/config.md
    -> .kool-ade-packet/config/project.json or project.md

.planner/mcp.json
    -> .kool-ade-packet/config/mcp.json

.planner/workflow.json
    -> .kool-ade-packet/state/workflow.json

adr/
    -> .kool-ade-packet/planning/decisions/
```

Historical root `SPECIFICATION.md` should be archived, not remain active.

## Remove Runtime Compatibility

Once migration is successful:

Delete compatibility code such as `task_dir()` choosing between old and new task trees.

Remove normal runtime reads from:

```text
planning/
.planner/
adr/
SPECIFICATION.md
```

Legacy paths should exist only in migration tests and migration code.

## Transaction Changes

Update `src/artifacts/transaction.rs`.

The transaction path allowlist must accept only canonical Packet artifact paths plus any explicitly approved special case.

Do not retain a primary transaction model based on `planning/` and `.planner/workflow.json`.

## Acceptance Criteria

- A fresh project creates no Packet artifact outside `.kool-ade-packet/`.
- An existing project migrates once.
- Restarting after migration does not attempt migration again.
- Migration interruption is recoverable.
- Conflicting legacy/target artifacts never overwrite each other.
- Task identities and implementation evidence survive migration.
- Queue state survives migration.
- Feature approval state survives migration.
- No runtime code depends on legacy project artifact locations.
- `git grep` for old locations finds only migration code, historical documentation, or tests intentionally exercising migration.

---

# 7. Phase 3: Fix Git Index Isolation

## Priority

Critical.

## Goal

Packet must never commit unrelated changes already staged by the user.

## Current Risk

Packet currently stages its intended paths but then executes a normal Git commit using the repository's shared index.

Any unrelated changes already staged by the user may therefore be included in a Packet-authored commit.

## Required Behavior

Packet must construct commits using only the exact paths authorized by the Packet transaction.

Use an isolated temporary Git index or another Git mechanism that guarantees path isolation.

The user's existing index must remain byte-for-byte logically equivalent after Packet finishes.

Packet must support:

- clean user index;
- unrelated staged changes;
- unrelated unstaged changes;
- untracked user files;
- partially staged files;
- Packet file overlapping a user edit;
- commit failure;
- cancellation.

A Packet planning commit must contain only Packet-authorized paths.

## Additional Git Fix

`gitops::stage()` must use process exit status as the success criterion.

Do not treat any nonempty stderr output as automatic failure if Git exited successfully.

## Required Tests

Create regression tests proving:

1. user stages `src/foo.rs`;
2. Packet changes only `.kool-ade-packet/planning/open-items.md`;
3. Packet commits;
4. Packet commit contains only its planning artifact;
5. `src/foo.rs` remains staged exactly as before.

Repeat with:

- partially staged changes;
- multiple unrelated staged files;
- Packet commit failure;
- Packet no-op commit.

## Acceptance Criteria

- Packet cannot commit unrelated staged user changes.
- Packet does not clear or alter the user's index.
- Packet commit paths exactly equal the transaction receipt.
- Git warnings on stderr do not falsely fail successful operations.

---

# 8. Phase 4: Replace Stringly Typed Workflow State

## Goal

Make orchestration state explicit and compiler-checked.

## Introduce Types

Replace status strings with enums such as:

```text
ImplementationStatus
PublicationStatus
PullRequestState
FailureKind
RecoveryDisposition
```

Example implementation lifecycle:

```text
Pending
Preparing
Implementing
Verifying
ReadyToPublish
Publishing
AwaitingReview
Blocked
Completed
Failed
```

Do not treat user-facing labels as persistent machine state.

## Replace Error Substring Logic

Remove logic such as checking whether an error contains:

```text
"diverged before publication"
"No implementation changes relative to the starting commit"
```

Instead persist structured failure data.

Example conceptually:

```text
Failure {
    kind: RemoteDiverged,
    message: "...",
    recoverable: true
}
```

Recovery decisions operate on `FailureKind`, not prose.

## Persistence

Serialized enum values must be versioned or migration-safe.

Existing persisted string states must continue to load through a one-time compatibility parser.

Do not permanently keep dual representations.

## Acceptance Criteria

- Core orchestration makes no decisions by matching status strings.
- Recovery makes no decisions by matching error message substrings.
- UI labels are derived from typed state.
- Existing saved implementation states migrate successfully.
- Invalid states are rejected or mapped deliberately.

---

# 9. Phase 5: Introduce Stable Internal Identities

## Goal

Separate identity from path, title, and human-readable numbering.

## Problem

Current task and change identity is heavily tied to:

- `CHG-nnn`;
- filenames;
- directories;
- task paths.

Historical CHG ID reuse already exists in the repository.

Path migration currently requires identity-repair logic.

## Target

Each durable planning entity should have:

```text
uid: globally unique immutable identity
display_id: human-readable stable project identifier
title: mutable display text
path: current storage location
```

At minimum apply this to:

- changes/features;
- task batches;
- tasks;
- ADRs;
- open items where useful.

Human IDs can remain:

```text
CHG-007
TASK-014
ADR-009
CLR-023
```

Internally they should not be the sole identity.

## Acceptance Criteria

- Moving a task file does not change its identity.
- Renaming a feature does not change its identity.
- A duplicate historical display ID cannot alias a current entity.
- Relationships refer to identities rather than filesystem paths wherever persistence matters.

---

# 10. Phase 6: Stop Recovering Machine State From Markdown

## Goal

Markdown remains the primary human-readable artifact but no longer doubles as an implicit database.

## Task Metadata

Introduce structured task metadata.

A simple Markdown front matter format is sufficient.

Example:

```text
---
uid: 01K...
id: TASK-014
change: CHG-007
repository: api
dependencies:
  - TASK-012
---

# Add token refresh handling
...
```

Packet owns and validates the metadata.

The LLM generates semantic task content but does not invent internal UIDs.

## Replace Current Parsing

Remove orchestration that extracts:

- dependencies from Markdown links;
- repository targets from arbitrary text lines;
- lifecycle identity from path shape.

Render dependency links for humans from structured data.

## Acceptance Criteria

- Dependency scheduling uses structured metadata.
- Repository routing uses structured metadata.
- Markdown formatting changes cannot silently alter execution semantics.
- Human-readable links still render correctly.
- Legacy tasks migrate or load through a bounded compatibility parser.

---

# 11. Phase 7: Redesign the Planner Decision Model

## Priority

High product value.

## Goal

Make recommendations, ramifications, and decision support first-class Packet behavior.

## Add a Decision Brief Domain Model

Introduce a reusable model approximately containing:

```text
DecisionBrief
  id
  question
  why_now
  recommendation
  rationale
  confidence
  options[]
  benefits[]
  costs[]
  risks[]
  ramifications[]
  reversibility
  defer_consequence
  evidence[]
```

Each option should support:

```text
Option
  id
  label
  summary
  benefits
  costs
  risks
  consequences
  reversibility
```

The exact serialization format can vary, but these concepts must be representable.

## Recommendation Rules

Packet should recommend a direction when enough evidence exists.

It must:

- distinguish recommendation from fact;
- explain why;
- retain alternatives;
- state uncertainty;
- identify evidence;
- avoid manufacturing precision.

Recommendation is advisory.

Human-authority decisions remain human decisions.

## Integrate With Open Items

A Review or Human open item may reference a Decision Brief.

Do not overload `OpenItem.recommendation: String` indefinitely.

The board should display a concise summary, with the detailed brief accessible when expanded.

## Extend Existing Attention Work

Refactor the existing implementation blocker explanation mechanism so its reusable concepts feed the same decision UI where sensible.

Implementation blockers may remain neutral when Packet lacks authority or enough evidence to recommend.

## Acceptance Criteria

- A consequential open question can present structured options.
- Packet can clearly identify its recommendation.
- Packet explains ramifications in plain language.
- Recommendation does not automatically resolve Human-authority items.
- Evidence is visible.
- Uncertainty is represented.
- Freeform user answers remain supported.

---

# 12. Phase 8: Replace Brittle Conversational Phrase Matching

## Goal

Remove hardcoded English command recognition from orchestration.

## Remove

Literal command matching such as:

```text
start implementing
begin implementation
implement the tasks
go ahead
yes please
generate task stories
```

from control-flow decisions.

## Target Model

Consequential operations must primarily be represented as typed application actions:

```text
ApproveChange
GenerateTasks
StartImplementation
PauseImplementation
ResumeImplementation
Publish
```

UI buttons are authoritative.

Conversation may also produce:

```text
requested_action
```

inside a structured response.

The application validates whether that action is currently allowed.

## Safety Rule

The model interprets what the user meant.

Rust decides whether the resulting action is legal.

A conversational request may never bypass a required human approval state.

## Acceptance Criteria

- "Looks good, let's build it" can be interpreted correctly.
- "Should we start implementing?" does not accidentally start implementation.
- Negations and conditionals remain conversation.
- Non-English requests are not inherently broken by English phrase tables.
- Existing explicit UI actions continue to work.

---

# 13. Phase 9: Replace Fixed Context Heuristics With Generative Retrieval Planning

## Goal

Preserve bounded context while improving relevance.

## Current Behavior To Replace

Avoid relying primarily on string heuristics such as:

```text
contains "F-"
contains "FR-"
contains "NFR-"
contains "D-"
```

or specific module filenames appearing in user text.

## Target Flow

Before the main planning turn, derive a small retrieval plan.

Example structured result:

```json
{
  "documents": [
    "product:architecture",
    "product:functional-requirements",
    "change:CHG-017"
  ],
  "openItems": ["CLR-021"],
  "repositoryAreas": ["src/auth", "src/session"]
}
```

The application then:

1. validates requested logical references;
2. applies budget limits;
3. loads authoritative sources;
4. passes those sources into the planning turn.

Retrieval selection is generative.

Context size and authority are deterministic.

## Avoid Premature Infrastructure

Do not add embeddings or vector databases unless later evidence proves they are needed.

Use logical document IDs, repository inspection, current change state, and model-selected relevance first.

## Acceptance Criteria

- Context remains bounded.
- Relevant documents can be selected without literal naming.
- The model cannot request arbitrary files outside allowed repository roots.
- Every retrieved planning fact points to an authoritative source.
- Historical feature growth does not cause unbounded prompt growth.

---

# 14. Phase 10: Make Specification Structure Adaptive

## Goal

Keep specifications concise without forcing every project into the same document ceremony.

## Preserve Required Concepts

A product specification must capture enough current truth to understand:

- what the product is;
- who it is for;
- what it currently does;
- major constraints;
- significant architecture;
- important current decisions;
- relevant quality requirements.

A change specification must capture:

- intent;
- current behavior;
- desired behavior;
- scope;
- requirements;
- decisions/assumptions;
- acceptance criteria.

## Remove Excessive Rigidity

Do not require every project forever to contain exactly thirteen product modules.

Do not require every product to contain sections that are irrelevant.

Allow project-specific modules such as:

```text
Security
Data Model
Deployment
Compliance
API Contracts
Migration
Performance
Accessibility
Billing
```

when warranted.

## Suggested Product Core

Keep a small required core, for example:

```text
Overview
Users and Outcomes
Current Capabilities
Architecture and Constraints
Decisions
Quality / Acceptance
```

Optional modules can then be created generatively as project complexity requires.

Exact final names may be chosen during implementation, but the key requirement is that layout becomes extensible.

## Validation

Rust validates:

- valid document identity;
- required metadata;
- duplicate IDs;
- allowed lifecycle;
- source containment;
- referential integrity.

The planner determines:

- which optional documents are useful;
- when they should exist;
- what belongs in each.

## Acceptance Criteria

- Small projects produce small specifications.
- Complex projects can grow additional modules without changing Packet code.
- Existing projects migrate without losing information.
- The UI can render arbitrary ordered product modules.
- Relevant modules can be retrieved individually.

---

# 15. Phase 11: Remove Arbitrary Task Word Counts

## Goal

Optimize generated tasks for completeness and clarity rather than size.

## Remove

Hard requirements such as:

```text
minimum 450 words
typically 700-1400 words
```

and similar section word-count thresholds used as proxies for quality.

## Replace With Semantic Completeness

A task must have enough information to safely implement its scope.

Validate structural requirements such as:

- clear goal;
- intent;
- target repository;
- affected area or files when reasonably knowable;
- dependencies;
- implementation constraints that actually matter;
- acceptance criteria;
- verification expectations.

Allow task length to scale with complexity.

## Optional Complexity Classification

The planner may classify:

```text
small
medium
large
high-risk
```

and adapt detail expectations accordingly.

This classification must not become another rigid ceremony.

## Acceptance Criteria

- A simple UI task can be concise.
- A dangerous migration can remain detailed.
- Padding does not make an otherwise incomplete task valid.
- Repetitive text is not rewarded.
- Existing task generation tests assert content completeness rather than total word count.

---

# 16. Phase 12: Redesign ADR Generation

## Goal

Produce concise architectural decision records rather than implementation transcripts.

## New Location

```text
.kool-ade-packet/planning/decisions/
```

## ADR Trigger

Create an ADR when planning produces a material durable decision such as:

- architectural approach;
- storage strategy;
- protocol choice;
- authentication approach;
- deployment topology;
- major third-party dependency;
- meaningful security tradeoff;
- significant consistency model;
- material operational constraint;
- decision that would be expensive to reverse.

Do not generate an ADR simply because a task was implemented.

## ADR Format

Use a compact format such as:

```text
# ADR-007: Use SQLite for local project state

Status: Accepted
Date: ...
Related change: CHG-012

## Context

Why a decision was necessary.

## Decision

What was chosen.

## Alternatives considered

- Option A
- Option B

## Why

Why this option fits the current project.

## Consequences

Benefits, costs, risks and constraints introduced by the decision.

## Revisit when

Conditions that should cause the decision to be reconsidered.
```

## Implementation Evidence

Move task verification, command transcripts, acceptance evidence and worker summaries to implementation evidence storage.

Do not embed them in ADRs.

## Existing ADR Migration

Preserve the existing large ADR files as historical evidence.

Do not destructively rewrite history.

Move or archive them under an appropriate historical location and begin generating the new format for future decisions.

## Acceptance Criteria

- Future ADRs describe decisions, not implementation tasks.
- ADRs are normally concise.
- ADR generation is tied to material decisions.
- Alternatives and consequences are retained.
- Historical implementation evidence remains accessible elsewhere.

---

# 17. Phase 13: Harden LLM Execution Boundaries

## Priority

Critical.

## Goal

Prompt instructions must not be the primary security boundary.

## Planning Mode

Normal planning should have read access to the connected repositories but should not have unrestricted write access.

If the harness supports a true read-only mode with repository inspection, use it.

If not, introduce an execution wrapper that exposes only the required read capabilities.

Do not rely solely on:

> Never modify repository files.

## Implementation Mode

Implementation workers may write only inside their assigned worktree and explicitly approved temporary areas.

The worker should not have unrestricted access to:

- the user's home directory;
- unrelated repositories;
- Packet's own process;
- credential stores;
- arbitrary system paths.

## Verification Mode

Verification commands must execute in the same controlled environment.

Apply limits for:

- wall-clock duration;
- child process count where practical;
- writable filesystem areas;
- environment variables;
- network access policy;
- disk usage where practical.

## Publication Mode

The agent does not receive Git publication credentials as general shell authority.

Packet itself performs:

- commit;
- branch integration;
- push;
- PR creation;
- merge;
- publication verification.

## Repository Content Is Untrusted

Treat:

- README;
- AGENTS.md;
- source comments;
- imported documents;
- task text;
- repository files

as untrusted input for the planning agent unless explicitly designated as instruction surfaces.

Repository content must not be able to escalate tool permissions.

## Acceptance Criteria

- A planning agent cannot modify the repository through available tools.
- An implementation worker cannot modify files outside its assigned writable boundary.
- Verification executes inside the same bounded environment.
- Git remote credentials remain under Packet control.
- Prompt injection from repository content cannot expand capabilities.
- Security guarantees remain valid even if the model ignores its prompt.

---

# 18. Phase 14: Split Automatic Planning, Building, and Publication

## Goal

Make automation understandable and safe for nontechnical users.

## Introduce Separate Controls

### Auto Plan

Packet may:

- investigate;
- maintain specifications;
- create planning items;
- produce recommendations;
- prepare change specifications.

Safe default:

```text
ON
```

### Auto Build

After explicit change approval, Packet may:

- generate tasks;
- run implementation workers;
- verify results.

Safe default may be:

```text
ON after explicit approval
```

or project configurable.

### Auto Publish

Packet may:

- integrate;
- push;
- create or merge PRs;
- update the remote default branch.

Safe default:

```text
OFF
```

The project may explicitly enable it after publication requirements are satisfied.

## UI Requirement

The user should be able to understand these states without knowing Git terminology.

For example:

```text
Plan automatically
Build approved changes automatically
Publish verified changes automatically
```

## Acceptance Criteria

- Approving a feature does not implicitly grant unlimited publication authority.
- Build automation and publication automation are separable.
- Current project policy is visible.
- Publication cannot occur when Auto Publish is disabled.

---

# 19. Phase 15: Add Independent CI and Quality Gates

## Goal

Do not let the same agent that creates a change be the sole judge that it is safe.

## Packet Repository

Add CI covering at minimum:

```text
cargo fmt --check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
```

Add appropriate platform coverage where supported.

## Eliminate Warning Baseline Debt

Do not permanently retain "no new warnings beyond ~110 existing warnings" as the standard.

Drive existing warnings to zero.

Then enforce:

```text
-D warnings
```

## Publication Policy

When a project uses a supported CI provider, Auto Publish should be able to require successful independent checks before final merge/publication.

Keep this policy pluggable because Packet will operate on projects beyond GitHub.

## Acceptance Criteria

- Packet itself has CI.
- Mainline commits cannot silently introduce formatting/test/clippy failures.
- Clippy warning baseline reaches zero.
- Auto Publish can distinguish local verification from independent verification.

---

# 20. Phase 16: Decompose Core God Modules

## Goal

Reduce hidden coupling and make lifecycle ownership obvious.

Do this after state and artifact migrations so the decomposition reflects the target architecture rather than preserving obsolete structure.

## `src/core/implementation.rs`

Split by lifecycle responsibility:

```text
src/core/implementation.rs

src/core/implementation/
  state.rs
  workspace.rs
  agent.rs
  verification.rs
  integration.rs
  publication.rs
  recovery.rs
  evidence.rs
  cleanup.rs
```

## `src/core/turn.rs`

Split conceptually into:

```text
turn/
  request.rs
  context.rs
  execute.rs
  validate.rs
  apply.rs
  controller.rs
```

Reuse existing modules where they already own the responsibility. Do not duplicate code merely to match this exact layout.

## `src/app/root.rs`

Move domain orchestration out of the UI root.

Candidate owners include:

```text
workspace_controller
planning_controller
implementation_controller
reconciliation_controller
```

Avoid creating generic "Manager" objects.

Each extracted component should own a real state machine or lifecycle.

## `src/app/dialogs.rs`

Split independent dialogs into their own modules.

## Size Rule

Reinstate the existing project guideline that source modules should remain approximately 300 lines where practical.

Do not blindly split cohesive code solely to hit a number.

## Acceptance Criteria

- Root UI code no longer owns implementation lifecycle mechanics.
- Implementation orchestration has clear submodules.
- Major lifecycle transitions have localized tests.
- Circular responsibilities are reduced.
- No replacement god abstraction is introduced.

---

# 21. Phase 17: Replace the Giant UI Surface Interface

## Goal

Simplify communication between application state and rendering.

## Target Direction

Prefer:

```text
Application State
      |
      v
WorkspaceViewModel
      |
      v
     UI
      |
      v
ApplicationCommand
      |
      v
Application State
```

Create typed commands such as:

```text
SendMainMessage
SendTaskMessage
AnswerDecision
ApproveChange
GenerateTasks
StartImplementation
PauseImplementation
CancelTask
OpenWorkspace
UpdateSettings
```

The UI should not need dozens of mutation methods exposed through one large trait.

## Migration

Do this incrementally.

Do not rewrite the entire UI in one pass.

Start with one cohesive area, likely task/board actions, then continue as value is proven.

## Acceptance Criteria

- UI reads primarily from view data.
- UI emits typed commands.
- Default no-op methods are substantially reduced.
- Application behavior is testable without rendering pixels.
- UI behavior does not become coupled directly to filesystem or Git operations.

---

# 22. Phase 18: Normalize Harness Capabilities

## Goal

Make the existing harness abstraction real.

## Replace Request Booleans

Avoid independent booleans such as:

```text
implementation
read_only
```

Introduce explicit execution modes or capability sets.

For example:

```text
Planning
ReadOnlyAnalysis
TaskGeneration
Implementation
Reconciliation
DecisionExplanation
```

Each mode defines permitted capabilities.

## Harness Injection

Do not instantiate `PiHarness` deep inside core modules such as attention/explanation handling.

Use the configured harness through dependency injection.

Pi remains the first supported implementation.

The architecture must not require Pi-specific changes outside the harness implementation.

## Capability Probe

Replace loose "any version whose `--version` works is acceptable" assumptions with capability detection where practical.

Check that required features exist, such as:

- JSON mode;
- thinking-level flag;
- tool-disable capability;
- event format expected by Packet.

Do not require version pinning merely for its own sake.

## Acceptance Criteria

- Core planning code does not directly instantiate Pi.
- Harness mode clearly expresses required capabilities.
- Invalid mode/capability combinations are impossible or rejected early.
- Future harnesses can be added without modifying planning domain logic.

---

# 23. Phase 19: Split Response Schemas By Operation

## Goal

Stop growing one giant envelope containing optional fields for unrelated operations.

## Introduce Operation-Specific Responses

Examples:

```text
PlanningTurnResponse
TaskOutlineResponse
TaskStoryResponse
ReconciliationResponse
DecisionBriefResponse
ImplementationReport
```

Use Serde structure validation before semantic validation.

## Migration

Support old schemas only as a temporary decoding layer.

Normalize immediately into the new internal response type.

Do not allow schema-version compatibility logic to leak throughout core behavior.

## Acceptance Criteria

- Task generation cannot accidentally contain planning-update fields.
- Planning responses cannot accidentally contain implementation-report fields.
- Semantic validators operate on operation-specific types.
- Legacy response versions are isolated to a decoder/migration layer.

---

# 24. Phase 20: Consolidate Planner Policy

## Goal

Reduce drift between Rust, prompts, and Markdown documentation.

## Separate Three Layers

### Machine Invariants

Implemented in Rust:

- IDs;
- legal transitions;
- filesystem containment;
- artifact schema;
- approval checks;
- routing authority;
- transaction safety;
- Git safety;
- execution capabilities.

### Planner Policy

One authoritative planner-policy source describing:

- how to interview;
- when to recommend;
- how to explain tradeoffs;
- when to create a decision;
- how concise to be;
- how to distinguish fact, inference, assumption, and recommendation;
- when to create an ADR;
- how to use current product truth and change specs.

### Project Knowledge

Stored under `.kool-ade-packet/`.

Do not copy machine invariants into prompt prose unless the model genuinely needs awareness of them.

Generate/inject planner instructions from the authoritative policy source rather than maintaining several independent prose copies.

## Acceptance Criteria

- There is one authoritative planner behavior policy.
- Application safety does not depend on planner compliance.
- Documentation accurately reflects machine behavior.
- Prompt and application policy cannot silently diverge in fundamental rules.

---

# 25. Phase 21: Improve Nontechnical Decision UX

## Goal

Ensure the architecture changes produce the intended user experience.

A decision card should answer, at a glance:

```text
What do I need to decide?

What does Packet recommend?

Why?

What happens if I choose it?

What are the realistic alternatives?

Is this easy to change later?

What happens if I do nothing?
```

Do not expose implementation vocabulary unless necessary.

Examples:

Prefer:

```text
This keeps all data on your computer.
```

over:

```text
Use SQLite-backed local persistence.
```

The technical explanation may be expandable beneath the plain-language explanation.

## Acceptance Criteria

- Every Human decision can be understood without reading the source code.
- Technical ramifications remain available.
- Recommendation is visually distinct from facts.
- The user can choose an alternative without fighting the planner.
- Packet does not ask several unrelated questions at once.

---

# 26. Phase 22: Harden Persistence and Atomic Writes

## Goal

Remove smaller filesystem reliability hazards.

## Work

Update generic atomic-write behavior so temporary filenames are unique per write.

Avoid a fixed sibling like:

```text
packet.tmp
```

Ensure:

1. write temporary file;
2. flush file contents;
3. rename atomically where supported;
4. consider syncing the parent directory for critical state;
5. remove orphan temporary files safely.

Review all direct uses of `std::fs::write` for durable state.

Classify each as:

```text
durable state
cache
diagnostic
temporary data
```

Only durable state requires full atomic semantics.

## Acceptance Criteria

- Concurrent writes do not collide on a shared temp filename.
- Critical Packet state cannot be left partially written.
- Recovery tests simulate interrupted writes.

---

# 27. Phase 23: Logging and Secret Hygiene

## Goal

Ensure diagnostics do not expose credentials or sensitive path material unnecessarily.

## Work

Audit logging and error construction involving:

- Git remote URLs;
- clone sources;
- MCP configuration;
- environment variables;
- harness command lines;
- external process stderr;
- repository paths.

Implement URL redaction for userinfo and embedded credentials.

Do not persist environment dumps.

Treat MCP configuration as potentially sensitive.

## Acceptance Criteria

- A remote URL containing credentials is redacted in logs/errors.
- Harness errors do not dump sensitive environment data.
- Diagnostic artifacts have intentional retention and location.

---

# 28. Final Artifact Contract

After remediation, the repository should have one obvious Packet-owned surface:

```text
.kool-ade-packet/
```

A user should be able to inspect that directory and understand the project planning state.

The rest of the repository is the project itself.

Packet should not scatter its project metadata around the repository root.

---

# 29. Final Behavioral Contract

The completed system should satisfy these principles.

## Planning

Packet proactively investigates before asking.

Packet asks only meaningful questions.

Packet explains why a decision matters.

Packet recommends a direction when appropriate.

Packet explains consequences and alternatives.

Packet records the decision durably.

## Specifications

Current product truth stays concise.

Proposed behavior stays in change specifications until implemented.

Specifications contain enough information to build and maintain the product, not ceremonial filler.

The specification structure can grow with the project.

## ADRs

ADRs explain durable decisions.

They do not duplicate implementation reports.

They remain concise enough to read later.

## Implementation

Only explicitly approved intent becomes implementation work.

Implementation happens in isolation.

Verification is bounded.

Publication is controlled separately.

## Reconciliation

Merged implementation is compared against approved intent.

Current product truth advances only after implementation evidence supports it.

Divergence becomes an explicit planning decision instead of silently rewriting intent.

---

# 30. Final Test Matrix

Before declaring remediation complete, the agent must prove at minimum the following scenarios.

## Fresh Project

- Connect empty/new repository.
- `.kool-ade-packet` is initialized.
- No legacy Packet directories are created.
- Initial planning works.
- Change spec is created.
- Decision recommendation works.
- Tasks can be generated.
- Implementation can be run.
- Publication policy is respected.
- Reconciliation updates current truth.

## Legacy Project

- Connect repository using old `planning/`.
- Migration succeeds.
- Existing product modules survive.
- Existing feature/change specs survive.
- Existing approvals survive.
- Existing open/resolved items survive.
- Existing tasks survive.
- Implementation state survives.
- Old paths are no longer used after migration.

## Migration Conflict

- Legacy and canonical copies both exist with differing bytes.
- Packet refuses to overwrite either.
- User receives a clear resolution path.

## Git Safety

- User has staged changes.
- Packet creates a planning commit.
- User changes do not enter Packet's commit.
- User staging state remains intact.

## Planning Security

- Repository contains prompt-injection text.
- Planner cannot gain write capability.
- Planner cannot perform publication operations.

## Implementation Security

- Worker attempts to write outside worktree.
- Operation is denied.
- Packet remains healthy.
- User repository outside the worktree remains unchanged.

## Decision Support

- User is presented with a meaningful architectural choice.
- Packet supplies options.
- Packet gives a recommendation.
- Packet explains rationale and ramifications.
- User selects another option.
- Packet accepts and records it without repeatedly arguing.

## Concision

- Very small feature produces a small spec/task.
- Complex migration produces appropriately detailed work.
- No artificial minimum word count forces filler.

## ADR

- Material architectural decision creates an ADR.
- Simple implementation task does not.
- ADR contains context, decision, alternatives, consequences and revisit conditions.
- Verification transcript is stored elsewhere.

## Recovery

Test interruption during:

- artifact migration;
- planning transaction;
- task generation;
- implementation;
- verification;
- publication;
- reconciliation.

Restart must either resume safely or stop in an explicit recoverable state.

---

# 31. Definition of Done

This remediation is complete only when all of the following are true:

1. `.kool-ade-packet/` is the sole home for shared Packet project artifacts.
2. Legacy artifact layouts are migration inputs only.
3. Packet cannot commit unrelated user-staged work.
4. Planning access is technically read-only rather than merely instructed to behave read-only.
5. Implementation and verification operate inside a defined execution boundary.
6. Publication authority is separate from planning/build authority.
7. Auto Publish is disabled by default.
8. Workflow states are typed.
9. Recovery logic does not depend on parsing human error strings.
10. Persistent task/change identity is independent of path.
11. Machine execution metadata is structured rather than recovered from prose.
12. Decision briefs are first-class planning objects.
13. Recommendations include rationale, consequences and alternatives.
14. Conversational control flow does not depend on fixed English phrase tables.
15. Context retrieval is relevance-driven but bounded.
16. Specification structure can adapt to project complexity.
17. Task quality is not measured by arbitrary word counts.
18. ADRs record decisions rather than implementation transcripts.
19. Core harness usage is implementation-neutral.
20. Response schemas are operation-specific.
21. Planner behavior has one authoritative policy source.
22. `root.rs`, `turn.rs`, `implementation.rs`, and other god modules have been decomposed around real responsibilities.
23. CI is active.
24. `cargo fmt --check` passes.
25. `cargo test --all-targets` passes.
26. `cargo clippy --all-targets -- -D warnings` passes.
27. Migration, Git safety, security, recovery and nontechnical decision-support scenarios have automated coverage.
28. The resulting user experience remains simpler than the architecture underneath it.

# 32. Implementation Order

Execute the remediation in this dependency order:

```text
0   Establish baseline

1   Centralize artifact layout
2   Migrate everything into .kool-ade-packet
3   Isolate Git commits from the user's index

4   Introduce typed workflow state
5   Introduce stable identities
6   Move orchestration metadata out of Markdown prose

7   Introduce first-class Decision Briefs
8   Replace phrase matching with typed actions
9   Add generative bounded retrieval

10  Make specification structure adaptive
11  Remove task word-count rules
12  Replace implementation ADRs with decision ADRs

13  Harden planning/implementation execution boundaries
14  Separate Auto Plan / Auto Build / Auto Publish
15  Add independent CI gates

16  Decompose god modules
17  Reduce the giant UI Surface abstraction
18  Normalize harness capabilities
19  Split operation response schemas
20  Consolidate planner policy

21  Finish nontechnical decision UX
22  Harden filesystem writes
23  Audit diagnostic and secret handling

24  Run the complete final test matrix
25  Remove superseded compatibility code
26  Update current product specification and ADRs to describe the final architecture
```

Do not begin broad module decomposition before the artifact, identity and workflow-state migrations are settled. Otherwise the implementation will spend significant effort cleanly modularizing architecture that is about to be deleted.