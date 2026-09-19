# CHG-005: Editable Operator Persona

**Status:** Ready
**Directed by:** Operator request in Main Chat: build out a persona system that tweaks the main agent's persona, editable in the interface using standard markdown, with a shipped default given by the operator (concise; protective of the user, the system, then the project; inquisitive; creative). Follow-up ruling: scoping is 'Operator level'. Boundary ruling (CLR-022): Option 1 — subordinate overlay.
**Bound:** Persona authoring, storage, and prompt injection only. Envelope schema, D-14 routing/veto, board protocols, worker queue, and the D-21 exit bar are untouched.

## Intent

Today the planner's voice is a constant compiled into the binary; tuning it means a source edit and a rebuild. The operator wants to own that themselves: a persona system authored and edited as standard markdown inside the interface, with a default embodying the operator's four beats. Scoping is ruled operator-level: the persona belongs to the seated operator locally — not to the project, not to the repository, not shared with collaborators. Success looks like: the operator opens the persona card, edits markdown, saves; the next turn — Main Chat or card conversation — already speaks the tuned voice, with no rebuild, no repository diff, and a reset that restores the shipped default. It matters now because CHG-003 elevated reply readability to a first-class surface, making voice the everyday contact — the operator wants to tune the instrument, not delegate it to the build cycle.

## Current Behavior

- The persona is `SYSTEM_INSTRUCTIONS`, a const at src/core/prompt.rs:12, joined per turn in src/core/turn.rs (the `system_instructions` format that appends SPECIFICATION_POLICY and the mode-specific tail) and handed to Pi as `--append-system-prompt` (src/harness/pi_harness.rs:233).
- One shared assembly feeds both conversation modes: Main Chat and per-card task conversations differ only in a trailing mode-note. The current voice therefore colors every agent utterance uniformly, by construction.
- No user-editable surface exists: no persona file, no settings card, no UI field. Closest precedents are the in-app MCP editor card in the Settings dialog (D-16/F-18, editing `.planner/mcp.json`; card painter in src/app/dialogs.rs) and the stakeholder/ownership card (F-17).
- `.planner/config.md` round-trip tolerates unknown sections but drops unrecognized prose on re-serialize (src/artifacts/config_io.rs; `tolerant_of_unknown_sections_and_empty_users` test) — it cannot carry a persona markdown body unmodified.
- Operator-local storage outside git already exists as a pattern: per-project `~/.packet/projects/<slug>/` stores (D-07/D-31, PACKET_HOME override), proving the home path for operator-level artifacts.
- The reply contract (envelope shape, routing legality, board law) is enforced application-side in src/core/validation.rs after the model completes — no prompt or persona text can waive machine legality.

## Desired Behavior

The boundary is ruled Option 1 (CLR-022): the persona is a subordinate overlay, not a wider override. The agreed shape:

- **Editor** — a **Persona** card in the Settings dialog: a standard-markdown text editor showing the current persona document (mirroring the MCP card's editing and save discipline), with a prominent **Restore default** action replacing the document with the shipped default, and a one-line notice that the persona tunes voice and principles while the application's protocol remains in force. Empty or unreadable files fall back to the shipped default at injection, with a diagnostic — never a silent blank.
- **Shipped default** — the operator's four beats ('Inqsitive' normalized to 'Inquisitive'; orthographic fix only):
  - Concise
  - Protective of the User, then the System, then the Project
  - Inquisitive
  - Creative
- **Operator-level storage** — the persona document persists in operator-local storage outside the repository (installation home, spanning projects; never committed, never synced into repos). Each machine/operator holds its own copy; cloning a project fresh changes nothing about the voice.
- **Injection** — the saved persona is injected per turn into the assembled system instructions as a labeled operator-persona layer after the standing contract, in both conversation modes, effective from the next turn.
- **Boundary** — RULED (Option 1, CLR-022): the layer tunes voice, priorities, and behavioral disposition only; the standing operating contract (envelope, routing/veto, board protocol, safety rails) stays in force, inviolate, and machine-enforced. Persona text may not legitimately request structural reshaping; machine legality is enforced application-side regardless of prompt text.

## Scope

In: the persona settings card (first-run seed, edit, save, restore default); operator-home file IO with fallback; per-turn injection covering both conversation modes; in-file unit tests (seed, save/load round trip, injection assembly, fallback, adversarial-boundary case); the D-34 no-new-warnings bar plus full regression.

Expected touch points: a seam in src/core/prompt.rs and src/core/turn.rs for loading and appending the layer; operator-home IO alongside the existing per-project stores in src/persistence/; the card painter beside the MCP card in src/app/dialogs.rs. No new crates; envelope and validation layers untouched.

Out: per-project or repo-committed/shared personas (would contradict the operator-level ruling); persona libraries/pickers, per-conversation overrides, version history; multi-operator profiles (D-23 seats one operator; storage may reserve a seam, not design one); distributing personas over the deferred D-18 channel; auto-suggested personas; letting persona text reshape formats, sections, or workflow habits (declined with the Option 1 ruling).

## Affected Product Areas

Updates land at reconciliation (no current-truth changes now):

- Module 03 (Actors and roles): persona authoring as an operator-local configuration act.
- Module 04 (Feature Inventory): new F-capability (projected F-26, next free number) once implemented.
- Module 05 (Functional Requirements): settings surface gains the persona-editor duty.
- Module 07 (Data Model): operator-home persona document record.
- Module 08 (Architecture): prompt-assembly seam in core; home IO in persistence; card in the settings dialog.
- Module 10 (Decisions Log): the operator-level scoping and the Option-1 boundary ruling recorded with the next free D-number at reconciliation.
- Module 13 (Source Map): persona rows.

## Requirements

Frozen at Ready; numbering follows CHG-003's convention.

- **REQ-P1-1 (MUST).** A standard-markdown persona editor in the Settings dialog: view, edit, and save the persona document; Restore default reinstates the shipped default; first run seeds the shipped default. The card presents the document as guidance beneath the standing contract.
- **REQ-P2-1 (MUST).** Persona persists at operator level — outside the repository, spanning projects, surviving relaunch — and is never written into any repository.
- **REQ-P3-1 (MUST).** The saved persona is injected into every planning turn's system instructions in both conversation modes, effective from the next turn; a missing or unreadable file falls back to the shipped default with a diagnostic.
- **REQ-P4-1 (MUST).** The persona layer is a subordinate, additive overlay (Option 1, CLR-022): it tunes voice, priorities, and disposition only. No persona text exempts the envelope, routing/veto, board protocol, or safety rails; application-side validation remains the sole legal authority.
- **REQ-ALL-1 (MUST).** D-34 bar met: full regression green, no NEW clippy warnings versus the baseline at the commit under verification on the pinned Rust 1.98 toolchain; no new dependencies.

## Decisions and Assumptions

- DE-1 (confirmed — operator ruling): scoping is operator level, hence operator-local storage outside the repository, spanning projects.
- DE-2 (confirmed — operator request): the shipped default is the four beats; 'Inqsitive' normalized to 'Inquisitive' as an orthographic fix.
- DE-3 (confirmed — operator ruling, CLR-022): the boundary is Option 1 — a subordinate overlay on top of the standing system contract. Persona text may legitimately request tone, priorities (user > system > project), and inquiring/creative disposition; envelope, routing/veto, board protocol, and safety rails stay inviolate. The wider-override option is declined: application-side validation already guarantees legality, so wider requests buy expressiveness only at the price of surprise fallbacks.
- AS-1: the persona colors ALL agent utterances (Main Chat and card chats) because both share one system-instruction assembly (src/core/turn.rs); per-surface voices are of scope.
- AS-2 (confirmed by DE-3): the overlay is additive and subordinate; the contract wins any conflict.
- AS-3: single-operator installation (D-23); one persona file; future multi-operator seam not designed here.

## Acceptance Criteria

Frozen at Ready:

- AC1: Settings → Persona shows the current document in a markdown editor; editing and saving persists; a relaunch shows the edited text and the repository diff stays clean.
- AC2: A pristine install seeds the shipped four-beat default; Restore default brings the document back to the shipped text.
- AC3: After save, the edited persona is observable in a Main Chat reply and in a card-conversation reply from the next turn on.
- AC4: Deleted or corrupt persona file → turns fall back to the default voice with a diagnostic; no crash, no silent blank.
- AC5: An adversarial persona instructing malformed envelopes or illicit routing claims → application validation still rejects the turn; contract duties hold verbatim.
- AC6: D-34 evidence: regression suite green; clippy baseline-diff neutral on the pinned toolchain; no Cargo.toml change.
