## Users and Outcomes

**Primary user.** A single developer-operator, the "chair seat," identified by Git identity (`user.name`/`user.email`; D-23 in `docs/exit-demo-runbook.md`). One seated operator acts per project checkout; collaboration channels are deliberately unbuilt (D-22 in the same document parks the MVP walk around them; the single-writer invariant NFR-3 assumes one app instance per project).

**Working context.**

- Local, potentially slow AI providers: planning turns default to a twelve-hour timeout, overridable with `KOOLADE_TURN_TIMEOUT_SECS` (`README.md`).
- Offline-oriented development: `cargo run --offline` with a pinned lockfile (`README.md`, `AGENTS.md`).
- Multi-repository capable: additional repositories are registered in `.koolade-packet/config/repositories.json`; local checkout paths stay operator-private (`docs/artifact-layout.md`). Today only `root` is registered.

**Intended outcomes.**

1. A trusted current product specification — six required concepts, kept concise, with Git carrying detailed history (living specification contract, `README.md`).
2. One board where questions, ambiguities, assumptions, ownership gaps, and implementation tasks coexist, with each decision routed to whoever is legally able to answer it (D-14 routing law, `src/core/routing.rs`).
3. A project manager that notices board events, helps eligible work continue, and makes user actions and external waits easy to spot.
4. Ordered, resumable task stories generated from approved specifications rather than ad hoc chat.
5. Implementation work verified in isolation, gated by explicit approvals (approve → generate stories → implement → publish), with durable evidence preserved.

**Ownership.** All stakeholder lanes in `.koolade-packet/config/project.md` are unconfigured; the seated operator inherits unowned lanes. If duties ever split, lanes should be named there rather than improvised in chat.
