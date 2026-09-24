# 001 — Add operator-home persona store with shipped four-beat default, first-run seed and corrupt-file fallback

Feature: Editable Operator Persona (markdown persona system with shipped default voice)

Status: Individually validated; see batch index for generation status.

## Problem this ticket solves and why

On this tree the planner's voice is a compile-time constant: SYSTEM_INSTRUCTIONS (src/core/prompt.rs:12) joins verbatim into every turn's system_instructions (src/core/turn.rs:255), so no operator-facing representation of the voice exists — no file, no loader, no saver. Left unresolved, tuning the persona still costs a source edit and a rebuild, and the two dependent tickets (Settings card story 002, per-turn injection story 003) would each have to invent their own file location and load semantics, risking exactly the per-project sprawl the operator-level ruling forbids.

## Ticket goal — what changes when done

Before: no persona exists on disk. After this ticket alone: an operator-level document lives at persona.md directly under the $PACKET_HOME-aware state root, is deterministically seeded with the shipped four-beat default on first encounter, save/load byte-round-trips arbitrary markdown across simulated relaunches, and a missing, deleted, blank, unreadable, or non-UTF-8 file yields the shipped default plus a returned diagnostic string instead of a blank, panic, or file mutation. Observable completion: the in-file battery passes under cargo test --lib persistence::persona on the pinned Rust 1.98 toolchain and a fresh temp home demonstrably gains a byte-equal persona.md.

## User story

As the seated operator Zachary Barno (sole seat per D-23), I want the planner's persona to persist as my own markdown file in my operator home, spanning every connected project, so that I can tune the voice with no rebuild and no repository diff, find it unchanged after relaunch, and get a diagnosed shipped default — never a blank or a crash — whenever the file is absent or corrupt.

## Purpose

The planner's voice is today a compile-time constant (SYSTEM_INSTRUCTIONS in src/core/prompt.rs), so tuning it costs a source edit and rebuild; this ticket moves the persona to operator-owned markdown at operator level in the existing $PACKET_HOME/~/.packet home (src/persistence/home.rs state_root), where it spans projects, survives relaunches, and can never enter any repository clone. It defines the shipped four-beat default (concise; protective of user, then system, then project; inquisitive — with 'Inqsitive' normalized; creative), seeds it deterministically on first encounter, and returns a diagnostic on a deleted or corrupt file instead of a silent blank, per REQ-P2-1/REQ-P3-1 and AC2/AC4.

## Specification references

Source: [Approved specification](specification.md)

## Implementation context

Verified at base 6214a58: state_root() in src/persistence/home.rs:10 honors $PACKET_HOME else ~/.packet; per-project stores key under projects/<slug>/, so a state-root-level file spans all projects and can never sit inside a repository clone; the only state-root artifact today is known_projects_path(); no persona-related code exists under src/ (grep-empty). House idiom (src/persistence/chat_store.rs): small public fns, io::Result returns, diagnostics carried as return values, atomic rewrites via crate::artifacts::atomic_write (src/artifacts/artifacts.rs:37 — parents created, temp file plus rename, temp removed on failure), and tests mutate the process-global PACKET_HOME only under a static ENV_HOME_LOCK mutex with pid-tagged temp dirs. Schema-less markdown means 'corrupt' realistically means invalid UTF-8, unreadable path, or blank. Constraints: no new crates; diff confined to src/persistence plus its registry file. Stated-unverified assumption: no sorted clippy baseline capture exists in-tree at this commit; the formal D-34 zero-added-lines diff runs in story 005 (recorded initial baseline: 110 warnings at 3ba5aa2).

Feature ID: CHG-003
Repository: root

## Technical design and contracts

- New module src/persistence/persona.rs exporting exactly: pub const SHIPPED_DEFAULT_PERSONA: &str; pub fn persona_path() -> std::path::PathBuf returning 'persona.md' joined DIRECTLY onto the state root computed by the $PACKET_HOME-aware home.rs helper (imported the way chat_store already consumes it), never under projects/<slug>/; pub struct PersonaLoad { pub document: String, pub seeded_now: bool, pub fell_back_to_default: bool, pub diagnostic: Option<String> }; pub fn load_persona() -> PersonaLoad; pub fn save_persona(document: &str) -> std::io::Result<()>.
- SHIPPED_DEFAULT_PERSONA normative bytes — eight lines, each terminated by a single LF, in exact order: the title line, a blank line, the intro line, a blank line, then the four bullets '- Concise', '- Protective of the User, then the System, then the Project', '- Inquisitive', '- Creative'. The title is a Markdown H1 reading 'Planner Persona (shipped default)'; the intro line reads 'Four standing beats:'. This is the operator's four briefed beats with 'Inqsitive' pre-normalized to 'Inquisitive' (orthography fix only); the constant is the single source of truth for seeding, story 002's Restore default, and every fallback-equality test assertion.
- load_persona() branch matrix (never panics, never propagates io errors): (a) readable, valid UTF-8, non-blank -> document is the EXACT file text with no trimming or newline normalization, both flags false, diagnostic None; (b) absent -> atomically write the constant and return it with seeded_now true plus a diagnostic stating that persona_path() was absent and the shipped default was seeded; (c) absent but the seed write itself fails (unwritable home) -> in-memory constant with fell_back_to_default true and a diagnostic citing the io error; (d) read io error (including the path being a directory), invalid UTF-8 detected via String::from_utf8 on the raw bytes (deliberately NOT from_utf8_lossy, which masks corruption), or blank via an all-whitespace check -> in-memory constant, fell_back_to_default true, diagnostic labelling the case 'unreadable', 'not valid UTF-8', or 'blank' respectively. Branch (d) NEVER rewrites or heals the file: corrupt bytes stay on disk as evidence. All diagnostics embed persona_path().display().
- save_persona(): preflight document.trim().is_empty() returns Err(ErrorKind::InvalidData, 'persona document must not be blank') before any disk touch, leaving the stored file byte-identical; otherwise durability delegates to crate::artifacts::atomic_write(&persona_path(), document) with the anyhow result mapped to io::Error kind Other. Stored verbatim — no BOM handling, no newline translation, no size cap (operator discretion, mirroring D-16's save-any-value MCP-editor discipline). Crash posture: persona.md is always either wholly the old document or wholly the new one; at most one stale temp file may linger in the home root and is displaced by the next successful rename.
- Diagnostic and concurrency posture: diagnostics travel ONLY in the returned Option<String>; the store emits no logging of its own (matching chat_store's corruption-reporting idiom), so tests assert exact substrings and story 002 displays the string in the Settings diagnostics area. Usage assumes one process and one active turn; a two-instance first-touch seed race writes the identical constant bytes through atomic rename, so last-writer-wins is a content no-op and determinism is preserved.

## Approved scope mapping

- Scope 2: Operator-local persona storage outside git (spans projects); fallback to the shipped default with a diagnostic on missing/corrupt file.

- Scope 5: In-file tests: seed, save/load round trip, injection assembly, fallback, adversarial boundary; D-34 regression bar.
- Success criterion 2: First run seeds the shipped four-beat default: concise; protective of the user, then the system, then the project; inquisitive; creative.
- Success criterion 3: Edits persist across relaunches at operator level — outside the git repository, spanning projects; repository clones carry no voice.

## Dependencies

None. This task can start independently.

## Affected files and components

- src/persistence/persona.rs (PROPOSED, new file): the whole operator-level persona store — the normative SHIPPED_DEFAULT_PERSONA bytes, persona_path(), PersonaLoad, the load_persona() seed/fallback matrix, save_persona() with its blank guard delegating to atomic_write, and the in-file #[cfg(test)] battery detailed in test_plan.
- src/persistence.rs (EXISTING): module registration — add 'pub mod persona;' beside the existing store-module declarations (chat_store et al.) plus the matching one-line module-doc bullet describing persona.rs as the operator-level persona document at the state root, preserving the registry's house style; no re-exports because consumers import the module path directly.
- src/persistence/home.rs (EXISTING, deliberately UNTOUCHED): consumed for its $PACKET_HOME-aware state-root computation only; altering home math would ripple through every per-project store and is outside this story's scope.

## Implementation steps

1. Register the module in src/persistence.rs: add 'pub mod persona;' alongside the existing store-module declarations and the matching doc-bullet line, so crate::persistence::persona resolves for later core/ and app/ consumers without touching src/lib.rs.
2. Create src/persistence/persona.rs opening with a crate-doc header stating the operator-level ruling, the persona_path() derivation from the state root (outside every repository by construction), and the schema-less markdown nature; define SHIPPED_DEFAULT_PERSONA with the eight normative lines, then implement persona_path() via the home.rs state-root helper.
3. Implement PersonaLoad and load_persona(): raw fs::read of persona_path(); the absent branch seeds the constant atomically with seeded_now true plus the absence diagnostic; route invalid UTF-8 through String::from_utf8's error; then the all-whitespace blank check; then the verbatim success arm; build every diagnostic with format! including persona_path().display().
4. Implement save_persona(): the blank preflight producing Err(InvalidData) before any disk IO; delegate to crate::artifacts::atomic_write(&persona_path(), document) and convert its anyhow error with map_err into io::Error::new(ErrorKind::Other, ...) so callers see plain io results.
5. Author #[cfg(test)] mod tests copying chat_store's idiom in substance: a static mutex guarding every PACKET_HOME env mutation, a use_tmp_home(tag) helper creating a pid-tagged dir under std::env::temp_dir() (removed first if present), setting the variable unsafely only under the lock, and deleting it in teardown; then the five test scenarios in test_plan asserting exact flag combinations, byte equality, file-touched/untouched evidence, and diagnostic substrings.
6. Prove bounded blast radius before finishing: run the filtered persona suite, then the full cargo test, then confirm via git status and git diff --stat that only src/persistence/persona.rs (new) and src/persistence.rs changed, with Cargo.toml and Cargo.lock byte-identical.

## Acceptance criteria

- Given a pristine $PACKET_HOME with no persona file, when load_persona() runs, then persona_path() exists with bytes exactly equal to SHIPPED_DEFAULT_PERSONA, seeded_now is true, and an immediate second load returns identical bytes with seeded_now false — the first-run seed is deterministic and idempotent.
- Given a seeded home, when save_persona() receives a custom multi-line document containing bullets, a two-pair-of-asterisks bold span, and an em dash, then two successive loads (simulating relaunch) return byte-identical text with fell_back_to_default false and diagnostic None, and no temp-residue files remain in the home root.
- Given a stored custom persona, when the file is deleted and load_persona() runs, then the file is re-seeded with the shipped default, seeded_now is true, fell_back_to_default stays false, and diagnostic names the absence together with the path — deletion never yields a blank and is never silent.
- Given persona.md overwritten with deliberately invalid UTF-8 bytes, when load_persona() runs, then document equals SHIPPED_DEFAULT_PERSONA, fell_back_to_default is true, diagnostic mentions UTF-8, and the on-disk bytes compare identical before and after the load — evidence preserved, no healing, no panic.
- Given a whitespace-only persona file, when load_persona() runs, then the fallback shape recurs with a diagnostic naming blank; separately, given any home holding a stored document, when save_persona() receives an empty string, a tab-plus-newline string, or spaces only, then an InvalidData error is returned and the stored document's bytes are unchanged.

## Test plan

1. Seed (fresh temp home): assert on-disk bytes equal SHIPPED_DEFAULT_PERSONA's bytes, seeded_now true, diagnostic present; a reload flips seeded_now to false with unchanged bytes; and the constant contains the words Concise, Protective, Inquisitive, Creative while it does NOT contain the misspelled variant Inqsitive.
2. Round trip (fresh temp home): save a multi-line document (nested bullets, bold marker pairs, an em dash, typographic quotes), load twice — both returns byte-equal to the saved input, fell_back false, diagnostic None — then scan the home root and assert zero leftover temp files.
3. Corruption/deletion matrix across three dedicated temp homes: (1) a file holding a byte outside the valid UTF-8 lead-byte range followed by junk => document equals the constant, fell_back true, diagnostic mentions UTF-8, and the file's bytes compare identical before and after the load; (2) a file holding only spaces and newlines => fell_back true, diagnostic names blank, file untouched; (3) a genuinely absent file => seeded on disk with seeded_now true and diagnostic present.
4. Save-gate negatives (fresh home pre-filled with a sentinel custom document): save_persona() called with the empty string, with a tab-plus-newline string, and with a triple-space string each return an error of kind InvalidData while the sentinel bytes remain unchanged; positive control: saving the sentinel returns Ok and reloads byte-equal.
5. Read-only home (unix-gated): seed, delete persona.md, chmod the home directory to read-only octal 500, assert load_persona() returns the constant with fell_back_to_default true and a diagnostic naming the write failure without panicking; teardown restores octal 700 and removes the temp dir so parallel suite siblings see a writable world.

## Verification commands and expected evidence

1. Working directory /mnt/DevProj/Packet: cargo test --lib persistence::persona. Expected: every persona unit test passes, 0 failed (host rustc/cargo 1.98 verified present at base 6214a58, matching the D-34 pin).
2. Working directory /mnt/DevProj/Packet: cargo test. Expected: the full suite — library units plus the multi_repository_feature and task_workflow integration targets — green with zero regressions against the base commit's recorded outcomes.
3. Working directory /mnt/DevProj/Packet: cargo clippy --all-targets piped through a filter for the word persona (e.g. rg -i persona over the combined output). Expected: EMPTY output, i.e. no warning names any symbol this story introduces; the formal D-34 gate (sorted clippy listing diffed against the pre-work capture showing zero added lines) is executed in story 005.

## Edge cases and failure handling

- $PACKET_HOME points at a read-only directory at first touch (file absent): required behavior is the in-memory shipped default with fell_back_to_default true and a diagnostic citing the failed seed write — no panic, no propagated io error, turns still voiced; the unix-gated test restores permissions in teardown.
- Two app instances race to seed a fresh home: both atomic renames carry the identical constant bytes, so last-writer-wins is a content no-op and no partial file can ever survive the rename.
- An operator legitimately saves a very large document (hundreds of KiB): save and load succeed with byte fidelity; there is deliberately no size cap or truncation (contrast chat_store compaction), so no quota logic may creep in under this ticket.
- A directory named persona.md exists at the state root: fs::read yields an io error, the unreadable fallback arm fires with its diagnostic, the directory is left unmodified, and the process continues serving the shipped default.

## Constraints

- Scoping is operator level — operator ruling; storage must live outside the repository.
- The shipped default is the operator's four beats (typo 'Inqsitive' normalized to 'Inquisitive').
- Boundary ruled Option 1 (CLR-022, DE-3): the persona is a subordinate voice-and-principles overlay; envelope, routing/veto, board protocol, and safety rails stay inviolate.
- No new crates; offline-minimal posture kept; envelope validation layer untouched.

## Out of scope

- Per-project or repo-committed/shared personas (contradicted by the operator-level ruling).
- Persona libraries/pickers, per-conversation overrides, version history.
- Multi-operator persona profiles (one seated operator per D-23; storage may reserve a seam).
- Distributing personas over the deferred D-18 channel; auto-suggested personas.

## Rollout and compatibility

Purely additive with no migration surface: there was no prior persona state to move, and because content is schema-less markdown no schema versioning is owed. Deployment-visible effect is one file appearing in the operator home (persona.md at the state root, honoring $PACKET_HOME or defaulting to ~/.packet) on first load — that appearance IS the feature working, and no repository working tree or .git directory is touched because the path derives solely from the state root. Rollback is reverting the two-file diff plus optionally deleting the orphaned persona.md; the worst steady-state degradation anywhere in the failure matrix is the shipped default voice, which is exactly the pre-feature behavior, so no blank or crash state is reachable from deployment.

## Definition of done

- cargo test --lib persistence::persona exits zero on the pinned Rust 1.98 toolchain, covering every test_plan scenario including all negative paths.
- Full cargo test suite is green at the working tree with no new failures against the base commit's recorded outcomes.
- git diff --stat shows exactly src/persistence/persona.rs (new) and src/persistence.rs (modified); Cargo.toml, Cargo.lock, and every connected repository working tree remain byte-identical.
- Workspace clippy output contains no warning naming a symbol this story introduces, keeping the delta clean for story 005's baseline-diff gate.

## Instructions for the implementing model

Read this story and its linked dependencies before editing. Confirm the named existing files and interfaces; proposed files are explicitly labeled. Implement only this task's scope, preserving the constraints above. Run the verification commands and record actual results, changed files, and any remaining limitations. Do not mark complete on a build alone when acceptance criteria require runtime interaction. If an assumption contradicts the repository or approved specification, report the discrepancy rather than silently inventing a requirement.
