## 12. Acceptance / Definition of Done

The binding MVP success scenario (D-20, ratified D-21) is a single operator conducting a live, repository-informed interview in a prepared git fixture. The resulting current specification improves, routed uncertainty appears, an answer becomes a recorded decision, and the git history shows a clean checkpoint chain. D-11 and D-22 keep this a single-workstation, single-actor demonstration; D-07/NFR-3 sharing questions belong to future collaboration.

Both legs are required:

1. **Behavioral demonstration.** Run `examples/prepare_exit_demo.rs` and the steps in `docs/exit-demo-runbook.md` against the live Pi harness. Verify seven outcomes: repository-grounded specification improvement; items in at least two categories; an ownership-gap nomination in the fixture's configuration (FR-7, D-25); one answer recorded as a decision with its item cleared; readable imperative git checkpoints (FR-3, NFR-9); a rejected invalid/misrouted envelope with byte-identical artifacts (F-6, FR-4); and cancellation without adopting fragments (FR-5).
2. **Automated invariants.** The full offline regression suite MUST pass, including routing, transactional recovery, task generation, implementation recovery, and UI behavior (NFR-8). Test counts are not an acceptance criterion and change as coverage grows.

The timeboxed real-team pilot and a standalone automated replacement for the live interview are excluded from this MVP bar. No new numerical performance/usability gate is implied (D-19). Only a fresh operator ruling can widen or weaken the bar.

CHG-001's separate 23-point feature acceptance list remains in its feature specification. That feature adds modular planning, authority, multi-repository tasks and reconciliation without silently altering this older MVP demonstration. F-19/F-20 board-display work (D-26, D-27) is also outside this bar; CLR-007 is resolved.
