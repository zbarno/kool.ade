# 005 — Author the fixture preparer and scripted runbook for the seven-outcome exit demonstration

Feature: Packet MVP — git-native desktop specification planner

Status: Individually validated; see batch index for generation status.

## Problem this ticket solves and why

The binding §12 exit bar defines MVP done as a live interview on a prepared, self-contained fixture repository walked through a fixed-step runbook, but no fixture preparer or runbook exists anywhere in the tree: there is no examples/, no scripts/, and no document mapping steps to outcomes (a)–(g). Left unresolved, the demonstration (later ticket) would be improvised, its checklist outcomes unverifiable in-session, and its evidence illegible in the resulting git history — turning the operator's binding definition of done into a coin flip that no reviewer could replay.

## Ticket goal — what changes when done

Two new artifacts exist and work: (1) `cargo run --offline --example prepare_exit_demo` deterministically authors a git fixture repository whose seeded planning state parses through the app's own strict loaders (the preparer self-verifies by reloading via `PlannerState::load` before exiting 0), and (2) `docs/exit-demo-runbook.md` maps every fixed step to outcome letters (a)–(g) with exact shell commands for in-session observation and git-history evidence. Observable completion: two consecutive preparations into fresh directories yield byte-identical planning/.planner/ contents, and a grep checklist confirms all seven outcomes are covered by named commands.

## User story

As the solo operator conducting the binding §12 exit demonstration, I want a one-command deterministic fixture preparer plus a fixed-step runbook with named evidence commands, so that I can perform (or anyone can re-perform) the seven-outcome demonstration reproducibly and certify each outcome from the fixture's own git history.

## Purpose

The binding exit bar's centerpiece presupposes a prepared, self-contained fixture repository and a fixed-step runbook, neither of which exists; without them the demonstration would be improvised, unreproducible, and its checklist outcomes unverifiable in-session or illegible in the resulting git history afterward. This ticket authors the deterministic seed preparer (hand-rolled temp-dir pattern, no new dependencies per NFR-7) carrying a git identity, naive planning state spanning at least two categories, an unowned category, and a seeded wrong claim, plus the runbook mapping every step to outcomes (a) through (g) with named git-history inspection evidence.

## Specification references

Source: [Approved specification](specification.md)

## Implementation context

Grounded in the inspected tree: artifact paths are the constants in `src/artifacts/artifacts.rs` (`planning/specification.md`, `planning/open-items.md`, `.planner/config.md`); `items_io::parse` is loud-strict (errors on bad CLR-NNN ids, missing fields, duplicates) and `PlannerState::load` (`src/core/state.rs`) fails the connect on an unparseable open-items.md, so seeds must be produced by the app's own serializers (`items_io::serialize`, `config_io::serialize`, `OpenItem::new` — all public, and `PlannerState` is publicly reachable, as `tests/task_workflow.rs` proves). `welcome::attempt_connect` only bootstraps MISSING artifacts and commits 'planner: initialize planning artifacts' only when it created something, so a fully seeded fixture incurs zero bootstrap commits and every later 'Packet Planner <planner@packet.local>' commit corresponds to exactly one accepted mutating turn (crucial for outcome (e)). Seats work via local repo git config (FR-13 reads the connected tree; `gitops` tests already use this local-config pattern); D-23 pins the seat to Zachary Barno/zbarno@gmail.com. The seeded `.planner/config.md` must OMIT any `## Current User` block or it would shadow the git-identity path. Seven default categories exist (`DEFAULT_CATEGORIES`, `src/domain/stakeholder.rs`); making InfoSec sole-owned by a fictional person ('Mira Voss') gives the fixture a live D-14 sole-ownership case. Fatal validation classes (`src/core/validation.rs`: unknown-id in open_items_resolved/open_items_updated, unrecognized priority/kind, blank spec) produce `TurnOutcome::Rejected{problems,..}` surfaced in chat as '⚠ Turn rejected — nothing was written.' plus a danger toast (`src/app/root.rs` ~line 190) — this is the outcome-(f) proof channel; an ineligible next_question_id is NON-fatal (dropped with a warning). Hand-rolled std tempdir patterns already exist in `spec_doc.rs`/`gitops.rs` tests; no `tempfile` or other dependency may be added (NFR-7; Cargo.toml lists only anyhow, chrono, egui/eframe, pulldown-cmark, serde/serde_json). NOTE discrepancy flagged for awareness, not changed here: code's turn-timeout default is 12 h (`turn.rs:29`, matches README) while spec F-15 quotes 2 h — the runbook cites the env-var behavior generically. Verified: `cargo check --offline` warning-free at /mnt/DevProj/Packet, and the git seed command sequence (init -b main; local config; add -A; commit) works in a /tmp smoke run.

## Technical design and contracts

- New file `examples/prepare_exit_demo.rs`, auto-discovered by Cargo (NO Cargo.toml edit, which also proves NFR-7 compliance); links only the `packet` library plus std. Entry `fn main() -> std::process::ExitCode`; `fn parse_args(iter) -> Args { dest: PathBuf, user: String, email: String }` taking an optional positional DEST (default `packet-exit-fixture` relative to cwd), flags `--user <NAME>` (default 'Zachary Barno'), `--email <ADDR>` (default 'zbarno@gmail.com'), `--help`; empty/whitespace user or email after trim is a usage error with exit code 2.
- Guards-before-writes contract: canonicalize DEST first; if the canonical path exists and is non-empty, or `DEST/.git` exists, print 'destination already populated or is a git repo — remove it or pass a different path' and exit 1 WITHOUT creating or modifying anything. All file content is fixed literals (no timestamps) so repeated runs are byte-identical; only git commit dates/SHAs vary, which the runbook explicitly scopes out of the determinism claim.
- Seeding helpers, each test-pure logic separated from IO: `write_project_files(root)` writes `README.md`, a 5-line `pyproject.toml` (name/version only), `tinypipe/core.py` (Pipeline.register/run executing transforms IN REGISTRATION ORDER, ~30 stdlib-only lines) and `tinypipe/cli.py` (UTF-8 stdin read — no decompression, no output cap — exit codes 0/1/2) via `packet::artifacts::atomic_write` (NFR-2 discipline); `seed_items() -> Vec<OpenItem>` constructs CLR-001 (Product/Ambiguity/Normal, unassigned), CLR-002 (QA/Question/High, unassigned), CLR-003 (InfoSec/Question/Normal, unassigned — sits in the sole-owned category, hence never poseable to the chair) via `packet::domain::OpenItem::new`, orders them with `items_io::sort_queue`, and serializes with `items_io::serialize`; `seed_config(user, email) -> String` builds a `PlannerConfig` with user = NONE (no '## Current User' block — deliberately forcing the git-identity seating path) and stakeholders = the seven `DEFAULT_CATEGORIES` all memberless EXCEPT InfoSec = ['Mira Voss'], serialized with `config_io::serialize`; `seed_spec_text() -> String` returns the naive tinypipe specification with exactly THREE seeded wrong claims, each refutable by reading the fixture code: (1) 'transforms execute in reverse registration order', (2) 'compressed input is transparently gunzipped', (3) 'output is capped at 1 MiB'.
- Git phase uses `std::process::Command` argument arrays exactly like `src/core/gitops.rs` (no shell interpolation): `git init -q -b main`, `git config user.name <user>` + `git config user.email <email>` (LOCAL config so the seat holds regardless of the operator's global git identity), `git add -A -- .`, `git commit -q -m 'chore: seed tinypipe fixture'`; any nonzero exit aborts with the offending command and stderr captured. The seed commit is authored by the SEAT identity, never 'Packet Planner', which is what keeps the post-connect checkpoint chain attributable to app turns alone.
- Self-verification phase (runs every invocation, fail-loud): reload the finished repo through the app's real loader `packet::core::state::PlannerState::load(&root)`; assert `state.items` equals the constructed seed vector, `state.config.user.is_none()`, stakeholders have seven categories with InfoSec as the only owned one, and `state.spec_text` is Some and starts with '# tinypipe'; additionally assert `items_io::parse(&written_bytes)` round-trips to the seed vector. Any mismatch prints the exact discrepancy (index/id/field) and returns exit 1. On success, print a MANIFEST block: fixture path, resolved `git config user.name/user.email` READ BACK from the new repo, the three seeded item lines (id|kind|category), and the runbook entry-point line.
- `docs/exit-demo-runbook.md` (proposed) fixed structure: Prerequisites gate (warning-free build, full NFR-8 suite green per the certificate ticket, pi discovered — verified in the settings dialog's 'Set up the pi harness' section landed by the guide ticket, git on PATH, `export PACKET_HOME="$PWD/packet-home-<date>"` for run isolation per the `$PACKET_HOME` override in `src/persistence/home.rs`); Steps 0–8 each with ACTION (verbatim chat prompts embedded), OBSERVED-IN-SESSION expectation, PROVES (letters a–g), and NAMED EVIDENCE COMMANDS — hash set `find . -path ./.git -prune -o -type f \( -path './planning/*' -o -path './.planner/*' \) -print0 | sort -z | xargs -0 sha256sum`, chain `git log --pretty='%h|%an|%ae|%s' -- planning .planner`, synthesis `git show <sha> -- planning/open-items.md | grep -F '+**Type:** Ownership'`, cleared-item `grep -c '^## CLR-00<n>' planning/open-items.md` → 0; an (a)–(g) sign-off table with an evidence-cell per outcome; and Retry rules: if the model refuses an induction, reinforce the next attempt by quoting the exact JSON fragment to embed, log attempts, and after three failed inductions REBUILD THE FIXTURE and redo the run — never hand-edit artifacts or cherry-pick hashes.

## Approved scope mapping

- Scope 1: Repository attach plus per-turn survey; one-active-turn interview chat with cooperative cancel and speaker-labeled log; read-only living-spec rendering with streamed live preview (F-1 through F-3, F-12).

- Scope 2: Typed open-items panel with priority grouping, kind/category/assignee badges, and ownership gaps visually distinguished (F-4); app-minted monotonic CLR item lifecycle (F-7).

- Scope 3: Turn pipeline with strictly validation-gated envelope application — zero mutation on any rejection — and exactly one imperative-subject git checkpoint per accepted mutating turn (F-5, F-6, F-9).
- Success criterion 1: Live multi-turn interview on a seeded fixture repository visibly improves the specification: sections earned, wrong claims corrected, code-grounded findings cited (checklist a).
- Success criterion 2: Items arise in at least two distinct categories with sensible assignments, and ownership-gap synthesis fires for at least one unowned category (checklist b, c; FR-7).
- Success criterion 3: At least one answer becomes a recorded decision with its item cleared and reflected in the spec (checklist d), and the checkpoint chain is intact with short imperative subjects (checklist e; FR-3, NFR-9).
- Success criterion 4: A deliberately invalid or misrouted envelope causes zero mutation — prior artifacts byte-identical, problems surfaced (checklist f; F-6, FR-4) — and cancelling an in-flight turn discards fragments cleanly (checklist g; FR-5).

## Dependencies

None. This task can start independently.

## Affected files and components

- examples/prepare_exit_demo.rs (NEW, proposed — does not exist today): the complete deterministic preparer — arg parsing, guards, project-file literals, seed builders, git driver, self-verification, manifest printer.
- docs/exit-demo-runbook.md (NEW, proposed — does not exist today): fixed-step runbook with prerequisites, Steps 0–8, verbatim prompts, outcome mapping table (a)–(g), evidence commands, retry rules.
- Cargo.toml (EXISTING, deliberately UNCHANGED): examples/ auto-discovery needs no manifest entry; the story fails review if any dependency or field is added here (NFR-7).
- src/** (EXISTING, deliberately UNCHANGED): no planner-code edits in this ticket; the preparer is a pure consumer of the public API (`artifacts`, `domain`, `core::state`).

## Implementation steps

1. Create `examples/prepare_exit_demo.rs` with `parse_args` (positional DEST defaulting to `packet-exit-fixture`; `--user`/`--email`/`--help`), canonicalize, then the exist-and-nonempty / .git-present guard that exits 1 before any filesystem write.
2. Implement `write_project_files` with the four tinypipe literals (README.md, pyproject.toml, tinypipe/core.py registering-and-running in order, tinypipe/cli.py with UTF-8-only stdin and exit codes 0/1/2), each written through `packet::artifacts::atomic_write`; keep every file under ~40 lines so the survey (`src/core/repo_overview.rs`, depth-2 tree, root manifests) shows the whole project.
3. Implement `seed_items` (CLR-001 Product/Ambiguity/Normal, CLR-002 QA/Question/High, CLR-003 InfoSec/Question/Normal, all unassigned, sorted + `items_io::serialize`), `seed_config` (no Current User; seven `DEFAULT_CATEGORIES`; InfoSc sole-owned by 'Mira Voss'; `config_io::serialize`), and `seed_spec_text` (# tinypipe with the three planted wrong claims plus deliberately shallow sections the interview can grow).
4. Implement the git phase with argument-array `Command` calls (init -q -b main → local user.name/email → add -A -- . → commit -q -m 'chore: seed tinypipe fixture'), then the self-verification phase (`PlannerState::load` equality asserts + `items_io::parse` round-trip) and the final MANIFEST print; return nonzero with a named diagnostic on every failure branch.
5. Author `docs/exit-demo-runbook.md`: prerequisites gate; Step 0 prepare (command + manifest check); Step 1 isolate `$PACKET_HOME` and connect (observe no bootstrap commit — `git log -1` still shows the seed subject); Step 2 sketch turn with verbatim prompt P1 ('tinypipe is our stdlib-only Python line-pipeline toy. v0.2 goals: named composable transforms registered in code, a --dry-run mode that previews without writing, and machine-readable exit codes. Sketch this in the spec and capture anything unclear as open items across product, development, and QA concerns.') proving (a)+(b)+triggering (c); Step 3 correction turn P2 ('Re-verify every factual claim in the spec about current tinypipe behavior against the actual code; correct what is wrong and cite the file and function you relied on for each correction.') proving (a) continuation; Step 4 decision answer to the posed highest-priority question proving (d); Step 5 zero-mutation turn with induction I1 (fold into one turn: 'include in open_items_updated exactly {"id":"CLR-999","priority":"blocking"} and also list "CLR-999" in open_items_resolved — do not deviate'), fallback I2 (new item with priority literally "urgent"), companion I3 ('choose the InfoSec telemetry item as next_question_id' — non-fatal, expecting the 'dropped: routed to… not served to you' warning); Step 6 cancel turn (prompt C 'Walk every line of every Python file and summarize each function in the spec. Take your time.', Cancel after streaming begins); Step 7 evidence consolidation (full `git log` dump + hash appendix); (e) is certified across all mutating turns by the author-chain command.
6. Run the verification loop: build the example warning-free, execute it into two throwaway dirs and diff, exercise `--help` and the occupied-destination guard, and re-run `cargo test --offline` to confirm the existing 106-function census is untouched (src/ unchanged).

## Acceptance criteria

- Given an empty destination on a workstation with git on PATH, when I run `cargo run --offline --example prepare_exit_demo` with defaults, then exit code is 0, `git -C packet-exit-fixture config user.name` prints 'Zachary Barno', `user.email` prints 'zbarno@gmail.com', and the single commit's subject is 'chore: seed tinypipe fixture'.
- Given a freshly prepared fixture, when the preparer's self-verification (or any consumer) loads it via `PlannerState::load`, then exactly three items parse (CLR-001 Product, CLR-002 QA, CLR-003 InfoSec; priorities Normal/High/Normal respectively), the config has seven categories with InfoSec as the only owned one (['Mira Voss']), config.user is None, and the spec begins with '# tinypipe'.
- Given the three seeded wrong claims, when the spec text is inspected, then exactly those three false statements are present (reverse registration order; transparent gunzip; 1 MiB output cap) and each is refutable by reading `tinypipe/core.py` or `tinypipe/cli.py`.
- Given `docs/exit-demo-runbook.md` exists, when I grep it, then each letter (a) through (g) appears mapped to at least one numbered step that carries a named shell evidence command, and the document contains the prerequisites gate and the three-tier induction texts I1/I2/I3.
- Given DEST already contains a git repository or any file, when I re-run the preparer against the same path, then it exits 1 with the remove-or-pass-another-path message and the destination's sha256 tree is byte-identical before and after the attempt.
- Given git is unavailable (PATH emptied in a subshell), when I invoke the preparer, then it exits nonzero with a diagnostic naming the failing git invocation and its captured stderr, and no partial planning artifacts remain that would parse-fail in `PlannerState::load`.

## Test plan

1. Determinism: run the preparer into /tmp/prep-a and /tmp/prep-b (sequential, defaults); `diff -r` over `planning/`, `.planner/`, `README.md`, `pyproject.toml`, `tinypipe/` reports zero differences; `git log --pretty=%s` subject lists are equal.
2. Load compatibility: after preparation, the preparer's own self-check (executing `PlannerState::load` + equality asserts) passes on every run; separately confirm `git -C fx log --pretty='%an|%ae' -1` shows the seat identity, NOT 'Packet Planner', proving the seed commit will not pollute outcome (e).
3. Guard negative path: create a dummy file in DEST, capture `find … | sort | xargs sha256sum` of DEST before the run, invoke the preparer expecting exit 1, re-hash and assert byte-identical.
4. Argument handling: `--help` exits 0 and prints usage listing DEST, --user, --email; `--bogus` exits 2 with a usage hint; `--user ' '` (whitespace) exits 2 naming the offending flag.
5. Suite integrity: `cargo check --offline` (already verified warning-free at baseline at /mnt/DevProj/Packet) stays warning-free with the example compiling, and `cargo test --offline` remains green at the known census with zero src/ modifications — the example adds no tests to the 106.
6. Runbook mechanical lint: grep the document for each of `sha256sum`, `%an|%ae|%s`, 'PACKET_HOME', and the seven letters (a)–(g) in mapping-table rows, plus the strings 'CLR-999' (I1), '"urgent"' (I2), and 'InfoSec telemetry' (I3) to prove the induction texts are present verbatim.

## Verification commands and expected evidence

1. cd /mnt/DevProj/Packet && cargo check --offline — VERIFIED on the current tree: 'Finished `dev` profile' with zero warnings; establishes the offline-compilation precondition the new example must also satisfy.
2. /tmp smoke (VERIFIED): `git init -q -b main && git config user.name 'Zachary Barno' && git config user.email 'zbarno@gmail.com' && git add -A -- . && git commit -q -m 'chore: seed tinypipe fixture'` in an empty dir — `git log --pretty='%an|%ae|%s' -1` returned 'Zachary Barno|zbarno@gmail.com|chore: seed tinypipe fixture'; this is the exact argument sequence the preparer will drive.
3. After implementation: cd /mnt/DevProj/Packet && cargo run --offline --example prepare_exit_demo --help — expect exit 0 and usage text naming DEST, --user, --email (command exists only once the example file lands; not runnable today).
4. After implementation: from a scratch parent dir, run the preparer twice into fresh siblings, then `diff -r prep-a/{planning,.planner,tinypipe,README.md,pyproject.toml} prep-b/…` — expect zero difference output; and inside the fixture `git log --pretty='%h|%an|%ae|%s'` shows exactly one commit, authored by the seat identity.

## Edge cases and failure handling

- Occupied/existing-destination DEST: guard probes before ANY write (check canonical dir emptiness and .git existence first) so a retried or mistargeted run can never leave a half-seeded repo behind.
- ~ -prefixed or trailing-slash DEST paths: canonicalize up front so the emptiness check, the .git check, and the printed manifest path all refer to the same real directory.
- Operator global git identity differs from D-23 (or is unset): the fixture's LOCAL user.name/email overrides global for all git reads/writes inside the fixture (smoke-verified in /tmp), so the seat is still Zachary Barno; the manifest reads the values back from the NEW repo to surface any anomaly (locked-down config, include.path weirdness).
- Parser drift between example and library: if a future refactor breaks `items_io`/`config_io` output, the preparer's built-in `PlannerState::load` round-trip fails at PREPARATION time with the exact field discrepancy instead of discovering corruption mid-demonstration.
- Model refusing every invalid-envelope induction during the live demo: the runbook's retry rule escalates by quoting the exact JSON fragment to embed, logs each attempt, and after three failed inductions mandates rebuilding the fixture from scratch — the zero-mutation hashes are never adjusted after the fact.
- Cancellation pressed before any stream event arrives: the turn is a clean no-op (cooperative cancel per F-5), no fragments to discard, and the post-cancel hash check still holds because nothing was ever applied.

## Constraints

- Pure Rust (edition 2024) with an offline-capable, minimal dependency graph — no socket or async crates at MVP; Packet initiates strictly zero network traffic (model and MCP traffic belong entirely to the operator-provisioned pi CLI's external configuration).
- Git is the sole durable store for shared planning state (no database); single-writer per process (concurrent submits refused); all planning-file writes atomic via temp-plus-rename; checkpoint subjects short and imperative; commit author Packet Planner.
- Clean harness seam: Packet orchestrates, the externally provisioned pi CLI performs all LLM interaction; discovery is PACKET_PI_BIN → PATH → common locations with a display-only, unpinned version policy (D-13).
- No quantitative performance or usability thresholds in the acceptance bar (ruled out by D-19); the §12 binding exit bar is alterable only by fresh operator ruling, never by quiet implication (D-21 anti-drift discipline).

## Out of scope

- The D-18 WebSocket real-time collaboration channel at MVP — deferred post-MVP as the first first-class post-MVP feature by D-22, carrying its open design questions CLR-015 (topology/trust), CLR-016 (writer model), CLR-017 (sharable surface vs D-07).
- Direct provider/API integration, embedded inference, and additional harness backends (seam retained via AiHarness; post-MVP intent per D-12); in-app harness installation automation (manual provisioning per the F-16 guide).
- Any platform or delivery vehicle beyond building and running from source on the operator's own Linux x86_64 workstation (other OSes, other architectures, installers, tarballs, packages — D-11, CLR-001 resolved).
- Spec editing in the UI, separate project databases, dashboards and analytics, ADR or risk registries, review/approval workflows, complex permissions or ACL engines, autonomous coding, PM-tool integrations (Contract §24).
- The timeboxed real-team pilot in the MVP exit bar (barred per D-11/D-20; reserved as first post-MVP dogfood) and a standalone scripted automated E2E harness (subsumed by the §12 demonstration plus the test suite).

## Rollout and compatibility

Zero blast radius: two new files, no src/ edits, no Cargo.toml changes, so the dependency graph (NFR-7), the warning-free build, the existing 106-function census, and operator workflows are all untouched; nothing migrates or initializes on normal app use because the example is never invoked by the binary. Land the preparer and runbook as a single atomic change so the doc's commands always match the preparer's flags. The runbook becomes ACTABLE only once the identity (git-derivation), routing (D-14 law), and in-app guide tickets have landed — its Prerequisites section says exactly that, and the document carries a 'blocked until' header line so no one conducts the demo prematurely. Rollback is simply deleting the two new files.

## Definition of done

- Preparer runs end-to-end on this workstation: exit 0, manifest printed, self-verification asserts passed, and a second preparation into a fresh dir yields byte-identical planning/.planner/content (diff output captured).
- Runbook present with prerequisites gate, Steps 0–8, all seven outcomes (a)–(g) each mapped to a step carrying a named evidence command, the I1/I2/I3 induction texts, and retry rules — verified by the grep lint in the test plan.
- `cargo check --offline` warning-free INCLUDING the new example, and `cargo test --offline` green at the unchanged census; `git status` shows exactly two added paths and no modified src/ files.
- Negative paths demonstrated once each: occupied-destination refusal (exit 1, destination unchanged) and missing-git diagnostic (nonzero exit naming the command and stderr).

## Instructions for the implementing model

Read this story and its linked dependencies before editing. Confirm the named existing files and interfaces; proposed files are explicitly labeled. Implement only this task's scope, preserving the constraints above. Run the verification commands and record actual results, changed files, and any remaining limitations. Do not mark complete on a build alone when acceptance criteria require runtime interaction. If an assumption contradicts the repository or approved specification, report the discrepancy rather than silently inventing a requirement.
