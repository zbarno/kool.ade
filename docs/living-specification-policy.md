# Living Technical Specification authoring policy

Maintain one authoritative Living Technical Specification for the connected project. It represents product intent, scope, actors and ownership, feature state, functional and non-functional requirements, data, architecture, environment and operations, accepted decisions, unresolved questions and risks, acceptance criteria, and implementation evidence. It is a coherent current document, never an append-only conversation or audit transcript.

## Maintenance contract

For every accepted planning turn:
1. Incorporate newly confirmed information into the appropriate sections. Return the complete replacement document when materially changed, or `null` when unchanged.
2. Update affected features, requirements, architecture, data, environment, and implementation statuses using repository evidence where available.
3. Record decisions, explicitly supersede conflicting decisions, and add, update, or resolve open concerns in both the document and the structured open-item changes.
4. Change the acceptance definition only upon an explicit decision; never silently widen or weaken an accepted bar.
5. Remove stale narratives, redundant facts, obsolete verification counts, and resolved concerns whose decisions already explain the outcome. Increment the specification revision for material changes.
6. Re-read the result as a coherent whole. Each revision must look intentionally authored in its current form.

Keep stable identifiers: `G-n`, `F-n`, `FR-n`, `NFR-n`, `D-nn`, and application-assigned `CLR-nnn`. Preserve existing spellings and numbers, even if they differ from these recommendations. Never renumber for appearance or reuse retired identifiers. Retain superseded decisions with an explicit reference to their replacement; do not silently remove prior rulings. Do not mint CLR identifiers yourself.

Distinguish confirmed requirements, observed implementation behavior, assumptions, provisional decisions, unresolved questions, and deferred work. Repository behavior is an observation until product intent confirms it. Never invent implementation state, decisions, ownership, metrics, acceptance evidence, or repository facts.

Inspect repository evidence before asking a human something the repository can answer. Integrate findings into their proper sections; record meaningful intent/implementation mismatches as open concerns, implementation deltas, or explicit decisions. Verify before marking a feature implemented or verified. Cite useful concrete paths, symbols, tests, artifacts, and commits without line-by-line audit transcripts.

Git is the long-term historical record. Retain historical explanation only when necessary to understand current behavior, rejected alternatives, binding constraints, or supersessions. Keep one authoritative location for each fact and reference it elsewhere. Prefer concise normative statements, short paragraphs, and deliberate MUST/MUST NOT/SHOULD/MAY language. Never let revision notes or resolved concerns overwhelm current truth.

## Required Markdown layout

Use exactly one H1: `# <Project Name> — Living Technical Specification`.

Start with a compact status block: specification version, maturity/status, document authority or precedence, relevant source/origin, and concise material changes in the latest revision. Follow immediately with a short **Maintenance** statement explaining how and by whom updates happen, whether direct editing is permitted, and how git preserves history. Packet's normal UI is read-only: the planning agent proposes full replacements and the application validates and persists accepted turns. Do not imply an in-app specification editor exists.

Use exactly these numbered H2 headings in this order. Put subsections at H3; do not add competing H2 headings.

## 1. Vision

Enduring purpose, central product or technical hypothesis, major goals, guiding principles, stable product/architectural tenets. Use stable goal IDs where useful. Keep detailed requirements elsewhere.

## 2. Scope

Separate in scope, out of scope, platform/runtime commitments, explicitly deferred capabilities, and important scope rulings. State intentional postponements clearly.

## 3. Actors and Roles

Human, system, and agent actors; responsibilities, ownership, authority, and routing rules. Separate the general model from concrete project assignments; use an ownership table where useful.

## 4. Feature Inventory

Stable `F-n` records: name, concise description, implementation status, relevant decision/requirement references. Prefer planned, partial, implemented, verified, deferred, or superseded. No implementation diaries; verified requires evidence.

## 5. Functional Requirements

Stable `FR-n` normative, externally meaningful, specific, testable behaviors. Avoid incidental implementation details unless required. Reference features/decisions instead of repeating their histories.

## 6. Non-Functional Requirements

Stable `NFR-n` constraints for reliability, atomicity, consistency, performance, responsiveness, security, privacy, deployment, portability, extensibility, maintainability, test quality, and history where applicable. State intentionally unspecified metrics; never fabricate quantitative targets.

## 7. Data Model

Use subsections as appropriate:
- `### 7.1 Artifact / Storage Layout`: table `Path / Store | Content | Writer / Owner`.
- `### 7.2 Core Domain Types`: entities, fields, enums, lifecycle rules, invariants.
- `### 7.3 Structured Protocols / Envelopes`: required/optional fields, validation, compatibility.
- `### 7.4 Local or Private State`: information intentionally outside shared storage.
- `### 7.5 Configuration`: structure, sources, precedence, defaults, serialization, fallback behavior.
Add further subsections for significant cache, wire, or external-state boundaries. Explicitly identify unknown or inapplicable details rather than inventing a design.

## 8. Architecture

Current layers/components and responsibilities first; interfaces and seams next. Use a compact hierarchy where useful and a numbered primary execution flow. State invariants, enforcement points, verified differences from intent, owed deltas, and reserved/deferred architecture. Keep current architecture separate from future concepts.

## 9. Environment, Launch, and Preconditions

Applicable platforms, runtime/toolchain, external programs, repository/identity/authentication assumptions, environment variables, local filesystem and external services, launch procedure, operational caveats. Separate hard prerequisites from soft assumptions.

## 10. Decisions Log

Use a table `ID | Decision | Basis | Status`. `D-nn` records state the actual ruling, not just a topic. Basis names operator decision, repository observation, existing contract, architectural constraint, or experiment evidence. Status: Proposed, Provisional, Confirmed, Observed / needs ratification, Superseded, or Retired. Retain replaced decisions marked superseded by the new ID.

## 11. Risks and Open Concerns

Active unresolved questions, ambiguities, assumptions requiring confirmation, architectural risks, deferred decisions and wake conditions, and technical debt. Reference stable open-item IDs. Separate active concerns, recently resolved concerns, and accepted debt; collapse old resolved history into its decisions.

## 12. Acceptance / Definition of Done

Concise success scenario followed by the binding acceptance criteria. Separate behavioral/demo evidence from automated/invariant evidence where applicable. Specify independently assessable required demonstrations, tests, artifacts/evidence, exclusions, deferred validation, and conditions for changing the bar. Preserve an already accepted definition unless explicitly changed.

## 13. Source Map

An evidence index: authoritative documents, source modules, tests, configuration, generated artifacts, implementation tickets, git/history, and normative agent/system instructions. Do not duplicate the architecture.

## Presentation

Markdown only; short paragraphs; H3 subsections for navigation. Tables for structured inventories, ownership, artifacts, decisions, and compatibility. Bullets for collections; numbered lists for ordered workflows. Backticks for identifiers, filenames, paths, commands, environment variables, and symbols. Bold sparingly. No decorative prose, giant paragraphs containing unrelated rulings, or repetitive revision diaries.
