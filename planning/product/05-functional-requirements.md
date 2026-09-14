## 5. Functional Requirements

- **FR-1** The specification file always holds the complete current document, never a delta.
- **FR-2** All specification changes originate from accepted agent turns; users cannot edit the spec directly (Contract §2).
- **FR-3** Every accepted mutating turn yields exactly one git checkpoint with a short imperative subject derived from the turn's change summary; non-mutating turns commit nothing (Contract §20).
- **FR-4** A failed validation causes no file mutation, no commit, and leaves prior state byte-for-byte intact; problems are surfaced to the operator (Contract §16).
- **FR-5** Cancelling a turn ends it promptly; streamed fragments are discarded; atomic writes make torn artifacts impossible.
- **FR-6** The agent may only pose questions eligible for the current user under the D-14 routing law: categorized General (structural broadcast); assigned to a group the user belongs to; belonging to a category explicitly owned by the user personally; or inheriting, by seat, a role with no explicit owner. Questions in categories sole-owned by another particular user are never posed to the seated operator; ineligible selections are rejected at validation; other stakeholders' items remain visible in the panel (Contract §11, refined by D-14).
- **FR-7** An open item in a non-General category with no configured owner forces an Ownership item to exist (synthesized on apply; Contract §8). Under D-14 the synthesized item requests a formal nomination; seat inheritance guarantees the lane is answerable in the interim. (Post-D-25 state: no unowned category remains in this repository, so synthesis is idle here.)
- **FR-8** Item IDs are unique, monotonic, minted exclusively by the application; retired numbers are never reused.
- **FR-9** pi runs with the connected repository as its working directory and may inspect it freely before asking humans for clarification; the app supplies curated context but does not reimplement repository comprehension (Contract §18).
- **FR-10** Imported documents stay in-repo and therefore remain natively inspectable by pi (Contract §21).
- **FR-11** In-memory planner state after an adoption exactly matches what was written to disk, even if the subsequent git commit fails (that error is surfaced, not hidden).
- **FR-12** Turn budgets: silence never shortens the deadline; zero/invalid env values fall back to the ruled default (12 h, D-24); the budget is fixed when the turn begins.
- **FR-13** The operator's identity is git-derived per D-14: `user.name` preferred, `user.email` fallback, the config's Current User block as tertiary source, `(guest)` last resort. Derivation runs at connection and after settings-save; git reads use the argument-array discipline (NFR-5) and are read-only invocations. [Landed with ticket 001 / PR #1 (`10f8e36`); v1.2 verified on master.]
