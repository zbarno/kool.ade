# 002 — Generalize task_chat::split_reply into a legacy-faithful digest detector for stored and new reply tails

Feature: Readable Chat Replies with At-a-Glance Asks and Quick Option Chips (CHG-003)

Status: Individually validated; see batch index for generation status.

## Problem this ticket solves and why

Whether a reply demands operator input is decided only by the private, card-surface-specific splitter task_chat::split_reply (src/ui/task_chat.rs:13), whose quirks — a global '**Your next step:**' flattening replace, first-occurrence split_once, and a 280-character cap measured on the un-trimmed line — are guarded by only two in-file test vectors. Main Chat rendering (src/ui/chat_pane.rs:188) has no notion of reply tails at all, so the upcoming digest grammar cannot be introduced without risking silent divergence of card behavior on stored legacy replies.

## Ticket goal — what changes when done

One shared classifier in new src/ui/reply_tail.rs parses raw pre-envelope reply prose into exactly five shapes (new digest, legacy 'Your next step:', final-'?' fallback, 'No reply needed.', plain) and task_chat delegates to it through a byte-identical adapter, so stored legacy replies classify byte-identically to today and the unreleased digest shape becomes machine-detector-ready; no painting changes. Completion: the new matrix tests pass, the pre-existing task_chat tests pass unmodified, clippy is warning-free.

## User story

As the seated operator relying on Packet to flag which reply wants an answer, I want one shared, unit-tested detector that classifies agent reply tails on every surface, so that recognizing 'input needed' can no longer disagree between Main Chat and card conversations and legacy stored replies never change meaning.

## Purpose

Whether a reply demands operator input is encoded in fragile string patterns understood only by the card-mode-specific split_reply, so Main Chat cannot recognize the same tails and any new digest grammar would break card behavior. This ticket introduces one shared detector (new src/ui/reply_tail.rs or message_text.rs extension) classifying raw reply prose — pre-envelope — into: new digest (final '---' rule plus ≤5 short bullet lines, at least one bullet), legacy 'Your next step:' tail (including bold-dressed), final-'?' fallback, 'No reply needed.', or plain/no-marker, with unit-test matrices proving byte-identical classification and lifted text against every stored legacy shape and the existing split_reply test vectors; no painting changes in this ticket.

## Specification references

Source: [Approved specification](specification.md)

## Implementation context

Verified at repository base d881038: split_reply(text: &str) -> Reply lives privately in src/ui/task_chat.rs (struct Reply { summary: String, next: Option<String>, no_reply: bool }, lines 9–45) and its single production caller is paint() at task_chat.rs:210, fed by message_text::readable(m) on the newest Agent message. readable (src/ui/message_text.rs:4) strips the final fenced JSON envelope (pi_extract::extract_json_object, src/harness/pi_extract.rs:12) or returns shield strings, so the detector always receives prose; stored turns persist the already-normalized assistant_message (src/app/root.rs:745). The legacy conventions were authored by the task-mode prompt (src/core/turn.rs:264: line beginning exactly 'Your next step:' / closing 'No reply needed.'); story 3 later rewords that prose, so legacy fidelity must hold here, now. Main Chat's paint_message (chat_pane.rs:188) wraps m.text as a plain label with no tail awareness. App-level fixtures reuse the same vectors (src/app/conversation_tests.rs:62: "SSO is recorded.\nYour next step: Should guests use SSO too?"). Proposed (NOT yet in tree): src/ui/reply_tail.rs and its registration line in src/ui.rs. Module registry src/ui.rs declares children alphabetically (lines 5–14: … overlays at line 9, spec_viewer at line 10); the new module slots between them. Constraint: no new crates, no Cargo.toml delta, no painting in this ticket, and the diff stays in src/ui/**.

Feature ID: CHG-002
Repository: root

## Technical design and contracts

- New file src/ui/pub(crate)-visible API, registered in src/ui.rs: #[derive(Debug,Clone,Copy,PartialEq,Eq)] pub enum TailKind { Digest, NextStep, FinalQuestion, NoReply, Plain } with manual impl Default { Plain }; #[derive(Debug,Default,Clone,PartialEq,Eq)] pub struct ReplyTail { pub kind: TailKind, pub body: String, pub ask: Option<String>, pub no_reply: bool, pub bullets: Vec<String> }; pub const MAX_DIGEST_BULLETS: usize = 5; plus private consts MAX_BULLET_CHARS: usize = 240 and MAX_QUESTION_CHARS: usize = 280; entry point pub fn parse_reply_tail(prose: &str) -> ReplyTail.
- Classification order is fixed and legacy-first: (1) flatten = prose.replace("**Your next step:**", "Your next step:") globally, then flat.split_once("Your next step:") → NextStep { body = pre-marker trimmed, ask = post-marker trimmed or None when empty, no_reply = false, bullets = [] } — case-sensitive, first-occurrence only, both locked by tests; (2) else if prose.trim_end().ends_with("No reply needed.") → NoReply { body = prose.trim_end().trim_end_matches("No reply needed.").trim(), no_reply = true, ask = None }; (3) else Digest: anchor = the LAST line whose trimmed text is exactly "---" (ASCII, strict; em-dash runs and *** / ___ are not anchors); all NON-BLANK lines after the anchor must be bullets and their count must be in 1..=MAX_DIGEST_BULLETS, failing any of which rejects the Digest outright (earlier anchors are never retried); (4) else the verbatim final-'?' fallback; (5) Plain.
- Bullet predicate, evaluated per line: trim_start, then begin with '-', '*', or '+' followed by at least one ASCII space/tab and at least one non-whitespace character, OR begin with one or more ASCII digits followed by '.' or ')' plus whitespace and non-whitespace; the captured bullet (marker and surrounding whitespace stripped, interior whitespace preserved) must satisfy chars().count() <= MAX_BULLET_CHARS. For Digest: body = the input BYTES spanning the start of the prose to the start of the anchor line, then .trim() (byte-span slicing, not line-rejoining, so CRLF and other interior bytes survive exactly as legacy summary derivation does); bullets collected in order; ask = Some(bullets[0].clone()) so a one-bullet digest maps 1:1 onto today's card 'Your answer needed' line.
- FinalQuestion path ports today's code verbatim: summary = prose.trim(); last_line = summary.lines().rev().find(non-empty after trim); if last_line exists, last_line.trim().ends_with('?') and last_line.chars().count() <= MAX_QUESTION_CHARS (measured on the UNTRIMMED line, exactly as task_chat.rs:34) → FinalQuestion { ask = Some(last_line.trim().to_string()), body = String::new() when summary == last_line.trim() else summary }; otherwise Plain { body = summary, ask = None, no_reply = false }. Legacy mapping into the untouched task_chat Reply struct is an identity transform: Reply { summary: tail.body, next: tail.ask, no_reply: tail.no_reply } reproduces current split_reply output bit-for-bit for all four legacy kinds — this equation is the compatibility contract asserted in tests.

## Approved scope mapping

- Scope 2: End-of-reply at-a-glance digest convention in the reply contract for all modes (main + task), a generalized detector replacing task_chat::split_reply with legacy 'Your next step:' and 'No reply needed.' compatibility, and the lifted digest block bringing Main Chat to the card chats' next-step standard.
- Success criterion 2: Any reply awaiting operator input ends with a compact, unlabeled digest (hairline rule + ≤5 bullets: ask, recommendation, pointer) rendered as a visually distinct block on both Main Chat and card surfaces; 'No reply needed' replies carry no digest.
- Success criterion 4: Stored legacy replies ('Your next step:' lines, final-'?' fallback, undecorated replies) behave identically to today, with the card's lift newly extended to Main Chat.
- Success criterion 5: Warning-free clippy/build, full regression suite green, no new dependencies; diff confined to src/ui/** plus two prose-only prompt paragraphs in src/core.

## Dependencies

None. This task can start independently.

## Affected files and components

- src/ui/reply_tail.rs (PROPOSED, new): the full classifier — TailKind, ReplyTail, parse_reply_tail, the bullet/anchor predicates, constants, and the in-file unit-test corpus (see test plan); no other file gains UI behavior from this ticket.
- src/ui.rs (existing): add exactly one line, pub mod reply_tail;, inserted between pub mod overlays; (current line 9) and pub mod spec_viewer; (current line 10) to keep the alphabetical module registry.
- src/ui/task_chat.rs (existing): replace the BODY of fn split_reply (line 13) with the two-line adapter mapping parse_reply_tail into the unchanged struct Reply; the Reply struct, the caller at line 210, and all painting/transcript code stay byte-untouched; append adapter-parity tests to the existing #[cfg(test)] mod tests.
- src/app/conversation_tests.rs (existing, no edits): regression net — its fixtures at lines 62 and 96 embed the exact legacy 'Your next step:' vector and a fenced-envelope assistant_message, so they must stay green unmodified as proof the card path behavior is unchanged.

## Implementation steps

1. Create src/ui/reply_tail.rs with the TailKind enum (plus impl Default yielding Plain), ReplyTail struct, the constants (MAX_DIGEST_BULLETS = 5, MAX_BULLET_CHARS = 240, MAX_QUESTION_CHARS = 280), and the doc comment stating the module classifies pre-envelope prose only and never rewrites message text.
2. Register the module in src/ui.rs by inserting pub mod reply_tail; between the overlays and spec_viewer declarations, preserving the file's alphabetical ordering and its //! header comments.
3. Transcribe the legacy paths first: perform the global '**Your next step:**' → 'Your next step:' replace, the case-sensitive first-occurrence split_once with the non-empty-filter on the post-marker text, the trim_end/end-with/trim_end_matches/trim chain for 'No reply needed.', and the reversed-lines final-'?' scan with the 280-char cap measured on the untrimmed line — copying task_chat.rs:14–42 expressions rather than paraphrasing them.
4. Implement the Digest detector per the fixed spec: strict trimmed-line-equals-'---' scan taking the LAST anchor, the blank-skipping all-non-blank-lines-must-be-bullets check, the 1..=5 count, the marker/character predicates, byte-span body extraction up to the anchor line, and ask = first bullet; rejection of a malformed tail must fall through to the FinalQuestion/Plain paths, never to a partial lift.
5. Rewrite split_reply in src/ui/task_chat.rs as the adapter (let tail = crate::ui::reply_tail::parse_reply_tail(text); Reply { summary: tail.body, next: tail.ask, no_reply: tail.no_reply }), touching nothing else in the file's painting code, and add adapter-parity tests asserting the digest mapping (body → summary, first bullet → next, no_reply false) alongside the two existing tests, which must not be rewritten.
6. Author the in-file corpus tests in reply_tail.rs per the test plan, then run the scoped test suites and the warning-free clippy gate; if any legacy vector diverges, fix the classifier to the old behavior — legacy output wins every conflict in this ticket.

## Acceptance criteria

- Given the stored reply "SSO is recorded.\nYour next step: Should guests use SSO too?", when routed through the task_chat adapter, then the result equals the legacy tuple NextStep with summary "SSO is recorded.", next Some("Should guests use SSO too?"), no_reply false — bit-for-bit the pre-existing test expectation.
- Given "SSO and MFA are confirmed.\nNo reply needed.", when parsed, then kind is NoReply, no_reply is true, body is "SSO and MFA are confirmed.", ask is None, and a trailing-newline variant ("...needed.\n\n") still classifies NoReply because trim_end precedes the endswith check.
- Given the new digest "Draft ready.\n\n---\n- Enable SSO for all guests?\n- Recommended: yes, effective Monday.\n- Impact notes: CLR-021", when parsed, then kind is Digest, body is "Draft ready.", bullets are exactly those three texts in order, and ask is the first bullet only.
- Given a reply whose final rule is followed by SIX bullet lines, when parsed, then kind is NOT Digest (it degrades to Plain with body = the full trimmed text and ask None) — no partial lifting of the first five bullets is permitted.
- Given "done.\nYour next step: pick vendor\n\n---\n- Vendor A", when parsed, then kind is NextStep (legacy markers outrank the digest grammar), locking parity for any future reply carrying both shapes.
- Given empty input "" or whitespace-only input, when parsed, then kind is Plain with body "" and ask None — no panic, no unwrap on missing lines — and given "a?\nb!" (question not on the final line) then kind is Plain, confirming only the FINAL non-empty line can win the fallback.

## Test plan

1. Table-driven corpus test in reply_tail.rs: a const array of (input, expected ReplyTail) covering every kind — at least the eight legacy shapes above (both task_chat vectors plus bold-dressed '**Your next step:**', empty-post-marker, doubled marker, lowercase negative, mid-phrase 'No reply needed.', bare final-'?' single-line and multi-line, over-cap 281-char question line constructed programmatically) plus the six digest/overflow/malformed-tail shapes — asserting ALL FIELDS (kind, body, ask, no_reply, bullets) per row, no partial matches.
2. Parity-guard test in task_chat.rs: for a fixed vector spanning legacy kinds, assert split_reply(input) == Reply { summary: tail.body, next: tail.ask, no_reply: tail.no_reply } where tail = parse_reply_tail(input), and assert the digest vector maps body→summary and first-bullet→next; the two EXISTING tests (next_step_is_separate_and_no_reply_is_not_a_request, conversation_progress_preserves_explicit_lifecycle_states) must pass with zero edits.
3. Boundary test for caps: build a 240-char and a 241-char digest bullet (assert Digest vs degraded), a 280-char and 281-char final '?' line (assert FinalQuestion vs Plain), and a 5-bullet vs 6-bullet digest (assert Digest vs Plain), generating the padding with iterators so lengths are exact.
4. Regression sweep: run the app-level conversation fixtures whose vectors embed the legacy grammar (src/app/conversation_tests.rs:62, 96, 391, 449) via the lib test binary and confirm they pass unmodified, proving paint-side needs_answer/heading behavior on cards is observably unchanged by the refactor.

## Verification commands and expected evidence

1. Working directory /mnt/DevProj/Packet (cargo 1.98.1 verified present; repo mandates offline builds): cargo test --offline --lib ui::reply_tail — expected: all new classifier tests pass, 0 failures.
2. Working directory /mnt/DevProj/Packet: cargo test --offline --lib ui::task_chat — expected: the two pre-existing tests plus the new adapter-parity tests pass, 0 failures, no test edits required.
3. Working directory /mnt/DevProj/Packet: cargo test --offline --lib conversation — expected: app-level conversation fixtures (embedding the legacy 'Your next step:' vector) pass unmodified.
4. Working directory /mnt/DevProj/Packet: cargo clippy --all-targets --offline — expected: completes with zero warnings (standing NFR-8 bar; also gates the whole batch).

## Edge cases and failure handling

- Interior horizontal rules and duplicates: a reply containing two '---' lines digests on the LAST anchor only, with the first rule swallowed into body bytes; a non-bullet line anywhere after the final rule (even one ordinary paragraph line) rejects the entire Digest and falls through to FinalQuestion/Plain — the trigger is malformed tail shape, the invariant is never lifting a fragment of a non-conforming reply.
- Unicode and casing traps: a final line of three em-dashes (—) is NOT an anchor (comparison is against the trimmed ASCII string "---"); lowercase 'your next step:' does not match NextStep (case sensitivity preserved); a marker occupying the whole string ("Your next step:" alone) yields NextStep with body "" and ask None, mirroring today's empty-post-marker filter.
- Stream-prefix degradation: during a live turn, callers may feed growing prefixes such as "Draft ready.\n\n-" or "Draft ready.\n\n---\n" — each must classify deterministically (Plain here) without panic, allocation explosion, or memoization state, because the classifier is pure and linear over chars; the invariant is idempotent, side-effect-free classification of arbitrary truncations.
- CRLF and interior bytes: a stored reply using \r\n newlines must yield bodies that are slices of the ORIGINAL input (line-rejoining is forbidden), so legacy summaries retain their exact interior bytes exactly as today's substring-derived sums do; the 280-char fallback cap counts chars on the untrimmed line, so a 280-char '?'-ending line with leading spaces passes while 281 chars degrades to Plain.
- Marker collisions: 'No reply needed.' occurring mid-paragraph without terminating the reply does NOT flip no_reply (ends_with on trim_end only), and a reply that both contains 'Your next step:' and terminates 'No reply needed.' classifies NextStep because the marker split is evaluated first — both behaviors reproduced verbatim from task_chat.rs:15–25.

## Constraints

- Offline minimal-dependency posture: no new crates; pulldown-cmark 0.13.4 is already a declared dependency and the spec viewer's Markdown→egui conversion proves the rendering path works in-tree.
- Standing NFR-8 bar: warning-free clippy/build and full regression suite green; diff confined to src/ui/** plus two prose-only prompt paragraphs.
- Existing documented stance that user-entered message text is plain text ('never Markdown-rendered from user input', chatlog.rs) — retained as an exclusion this turn.
- Digest and chips are derived solely from assistant_message prose BEFORE the final fenced JSON envelope; the envelope remains the only machine contract, and pi_extract's 'last fenced json wins' rule is untouched (invariant-tested).
- Scope concession recorded: for the first time a display feature touches src/core (two prose-only prompt paragraphs) because the digest/chips depend on model output shape; no logic or semantic changes.

## Out of scope

- Markdown rendering of user-entered messages (they stay plain per the documented chatlog.rs stance; one-word re-open later).
- Auto-send from chips, chip keyboard navigation, animation, selectable themes or fonts.
- Spec viewer or other surfaces (already Markdown-rendered) and Markdown capabilities beyond the core engine set (headings/emphasis/lists/code/links/tables as the engine already yields).
- Envelope schema, routing/veto, streaming-timeout/cancel semantics, or persistence-format changes; zero src/harness changes.

## Rollout and compatibility

Pure display-layer classification: no persistence, schema, prompt, envelope, or harness change, so stored conversation bytes are untouched and no migration or re-seeding exists. Rollback is trivially reverting the three-file diff (delete src/ui/reply_tail.rs, drop one line from src/ui.rs, restore the split_reply body) with zero state recovery owed. Old replies remain interpretable indefinitely because the legacy grammar stays supported forever; story 3's prompt rewording lands separately and the detector accepts both grammars concurrently during the transition. Concurrency/cancellation are irrelevant — the parser is pure and allocation-light.

## Definition of done

- Diff is confined to src/ui/reply_tail.rs (new), src/ui.rs (one module line), and src/ui/task_chat.rs (adapter + added tests); Cargo.toml, src/core, src/harness, and src/app are byte-identical.
- The corpus matrix exercises all five TailKind values and locks every legacy quirk (case sensitivity, first-occurrence split, untrimmed 280-char cap, trim chains) as explicit passing assertions.
- All four verification commands finish green with clippy emitting no warnings, and the two pre-existing task_chat tests plus the app conversation fixtures pass without any edit.

## Instructions for the implementing model

Read this story and its linked dependencies before editing. Confirm the named existing files and interfaces; proposed files are explicitly labeled. Implement only this task's scope, preserving the constraints above. Run the verification commands and record actual results, changed files, and any remaining limitations. Do not mark complete on a build alone when acceptance criteria require runtime interaction. If an assumption contradicts the repository or approved specification, report the discrepancy rather than silently inventing a requirement.
