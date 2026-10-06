## Architecture and Constraints

**Stack.** Rust edition 2024, package `koolade` v0.1.0 (MIT). Deliberately minimal dependencies: `egui`/`eframe` 0.36 for the GUI, `anyhow`, `chrono`, `serde`(+`serde_json`), `uuid`, `pulldown-cmark`, `rfd`, `image` (PNG only) (`Cargo.toml`). The release profile uses thin LTO.

**Module map** (`src/lib.rs`; each module is kept around 300 lines or less):

- `domain` — value types: open items, stakeholders, current user, chat log
- `artifacts` — planning file IO, migration, transactions, product/task documents
- `persistence` — out-of-Git runtime state (chat history, task conversations)
- `harness` — `AiHarness` trait plus the Pi CLI implementation (the only AI backend)
- `core` — planning engine: state, routing, validation, apply, gitops, turn pipeline, task generation, implementation queue, reconciliation, workflow
- `app` — eframe session wiring around the `KooladeApp` state machine
- `ui` — egui presentation behind the `Surface` trait; views emit typed `ApplicationCommand` intents rather than touching state directly (`src/ui.rs`)
- `diagnostics` — local panic and startup error reports

Style constraints (`AGENTS.md`): directory module layout, never `mod.rs`, split modules before they exceed roughly 300 lines.

**Data planes.**

- Shared, Git-backed: `.koolade-packet/` — config (`project.md`, `repositories.json`, `mcp.json`), planning (product modules, changes, tasks, open/resolved items, decisions, imports, archive), `state/workflow.json`, and the Git-ignored `implementation/` evidence (`docs/artifact-layout.md`).
- Operator-private: `~/.koolade-packet/projects/<slug>/` or `$KOOLADE_HOME` — chat, task conversations, resumable generation checkpoints, and the local checkout-path mapping.
- Git common-directory metadata: per-clone process locks, migration/transaction journals, implementation queue and task state, private refs, Pi event streams. The queue lock prevents competing windows in one clone; temporary atomic `refs/heads/koolade/claims/<task-uid>` claims coordinate implementation across independent clones through a writable, reachable `origin`. Active workers refresh claims every five minutes; claims appear stale after fifteen minutes without a refresh and support explicit stale-only compare-and-swap takeover. They do not enable collaborative planning.

**Runtime constraints.**

- Single-writer per clone: queue and publication locks arbitrate across windows sharing a Git common directory. Independent clones coordinate implementation of the same task through remote claims. With no configured origin, manual and automatic work remain usable with a visible local-only warning. If a configured origin is unreachable, an explicit manual start may proceed with that warning while automatic work waits for remote coordination (`docs/artifact-layout.md`).
- The AI boundary is a Pi CLI child process only; no embedded model. Turn timeouts default to twelve hours and are fixed when a turn begins (`README.md`).
- Autonomous capabilities (sandboxed planning reads, investigation, implementation) require Bubblewrap on Linux x86_64; without it those modes stop with setup guidance while the planning UI remains available. The provider relay accepts only a private HTTP OpenAI-compatible endpoint; public/HTTPS endpoints fail closed (`README.md`, Host execution capabilities).
- Environment knobs: `KOOLADE_HOME`, `KOOLADE_TURN_TIMEOUT_SECS`, and `KOOLADE_PI_BIN` (discovery order documented in `docs/exit-demo-runbook.md`).

**Safety properties exercised by tests.**

- No artifact is mutated without a validated structured response, and bad document IDs are rejected atomically (chg-001 AC2, AC12).
- Interrupted multi-document writes restore original bytes; accepted sets are Git-checkpointed (chg-001 AC12).
- Chat-question selection and board mutation enforce the D-14 routing/veto laws (`src/core/validation.rs`).
- Plan adoption is explicit; approval fingerprints cover the adopted plan (`.koolade-packet/state/workflow.json`).
- Publication requires checks against the exact verified commit; failed verification never advances the remote (`README.md`).

**Operational constraints.**

- Quality gates pin the 1.98.1 toolchain: `fmt --check`, the single-threaded full test run, and Clippy with `-D warnings` (`AGENTS.md`).
- A 1 GiB minimum available-disk guard precedes implementation and verification (a start guard, not a reservation); the 2026-09-22 disk-exhaustion incident and 2026-09-23 cleanup are documented in `docs/task-failure-diagnosis.md`.
- Test process cleanup must use `scripts/owned_gui_processes.py` (unique run marker, exact executable, ancestor exclusion, pidfd signaling); machine-wide name matching is forbidden (same document).
