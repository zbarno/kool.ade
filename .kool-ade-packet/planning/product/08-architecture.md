## 8. Architecture

Packet is organized as `ui → app → core → harness`, with `domain`, `artifacts`,
and `persistence` as shared boundaries. The UI renders conversation, the modular
specification, the Kanban, task details, and activity. It sends typed commands to
the app through a small dispatcher; task details consume a read-only view model.
The app owns sessions, dialogs, and worker/queue coordination. Core owns bounded
context selection, typed workflow decisions, validation, recoverable artifact
transactions, task generation, implementation, publication, and reconciliation.
The harness owns external model-process capabilities and supervision, not project
truth.

The connected repository's shared Packet files live under `.kool-ade-packet/`.
Migration runs before the application loads project state; afterward, normal
runtime reads and writes use the canonical artifact layout. Local conversations
and generation checkpoints live under `PACKET_HOME`; queue state, implementation
state, locks, journals, private Git refs, and Pi event streams use Git's common
directory. Implementation reports and verification evidence live under the
ignored `.kool-ade-packet/implementation/` directory.

Planning context starts with the authoritative planner policy and a small project
orientation. A bounded retrieval pass selects logical product documents,
open-items, and repository areas; Rust resolves selections against the live
catalog, checks containment, and applies per-source and total budgets. Completed
history and unrelated task conversations are not loaded by default. Task chat
uses the item's own durable content and evidence and remains isolated from Main
Chat and other tasks.

Each harness request carries a typed operation mode. Planning, task generation,
and investigation get read-only repository tools; analysis, reconciliation, and
decision explanation get no tools; implementation and verification receive only
Packet's bounded shell tool inside the assigned worktree. The Pi adapter probes
its CLI flags and JSON/event capabilities before execution and rejects missing
requirements. Other harnesses can implement the same boundary but are not shipped.
Wire responses use strict operation-specific schemas; Rust normalizes them into
internal workflow types before applying operation-specific semantic checks.

Planner behavior and authoring rules come from Packet's `docs/planner-policy.md`.
Rust enforces identity,
schema, legal transitions, containment, approvals, routing, transaction safety,
Git safety, and tool capabilities independently of prompt compliance.

For Ready features, Compare Plans produces exactly two validated candidates and
an advisory recommendation. The versioned comparison record is workflow state;
adoption writes the operator's selected plan into the feature contract, creates
an ADR when the choice is material, and invalidates approval before explicit
re-approval against the new fingerprint.

An approved feature freezes the affected product modules, repository heads, and
configuration into its task-batch contract. Implementation runs in isolated
worktrees and verifies before any sharing. Auto Plan, Auto Build, and Auto Publish
are independent project controls: Auto Plan can investigate Agent-owned items;
Auto Build continues explicitly approved work; Auto Publish is off by default
and is required for unattended integration into a remote default branch. With
Auto Publish off, verified work stays local until the operator chooses to create
a pull request. Publication is serialized per repository; dependent tasks wait
for their prerequisites to merge. Reconciliation reads actual merged commits
before changing current product truth; a material disagreement becomes a visible
Review or Human item.

Packet currently supports Linux x86_64. Planning, task generation, investigation,
implementation, and verification require an operational Bubblewrap sandbox.
Planning uses read-only repository tools; implementation and verification use
Packet's bounded shell inside the assigned worktree. When Bubblewrap is missing
or cannot establish its namespaces, Packet stops before starting Pi and raises a
board setup item with a retry action. It does not fall back to an unsandboxed Pi
process. The planning profile hides credentials and isolates network access. A
host-side relay supports only configured private/local HTTP OpenAI-compatible
providers. Unsupported provider configurations fail closed and produce their
own board setup item with a recommendation and repair steps.

Architectural invariants: Git-backed artifacts outrank model recollection; board
state is a projection; repository targets are verified by stable identity; no
atomic transaction is promised across repositories; and product truth does not
advance before merged-code reconciliation. The deferred collaboration channel
remains future work (D-18, D-22). D-31 limits its eventual shared surface to
planning artifacts and presence, D-32 fixes the direct-peer topology and trust
ground, and D-33 requires a convergence layer for true concurrent co-authoring.
