# 001 — Factor spec-viewer cmark fold into dark painter and format agent replies in Main Chat, cards, stream

Feature: Readable Chat Replies with At-a-Glance Asks and Quick Option Chips (CHG-003)

Status: Individually validated; see batch index for generation status.

## Problem this ticket solves and why

Today every agent reply paints as one unstyled wrapped egui::Label with literal '**', '#' and triple-backtick glyphs in Main Chat (chat_pane.rs:222), in card-tab 'Conversation history' transcripts (task_chat.rs:100), and in live-streaming reply chunks (chat_pane.rs:234ff) — the operator reports the resulting text wall hides what the agent needs. The spec viewer already proves a pulldown-cmark 0.13 to egui fold works in-tree, but it is private and hardwired to the light paper palette, so chat surfaces cannot reuse it until the fold is factored into a theme-parameterized dark painter; until then every reply surface stays unstructured and hard to skim.

## Ticket goal — what changes when done

After this ticket alone, every agent reply — stored transcripts on Main Chat and card tabs, plus the in-flight streaming reply text — paints as structured dark-theme Markdown: headings, bold/emphasis, lists, quotes, rules, inline and fenced code, and tables render as styled spans with no visible raw syntax markers, while user bubbles, System notices, Thinking/Tool collapsibles, and the spec viewer's white paper stay behavior-identical. Observable completion: egui shape tests on a canned Markdown reply assert styled sections and zero marker glyphs, the two incumbent paper tests pass untouched in substance, and the full regression suite (292 tests at d881038) is green with no new dependencies.

## User story

As the seated operator reading planner replies in Main Chat and per-card task conversations, I want agent replies painted as structured dark-theme Markdown (headings, bold, lists, code) instead of a marker-riddled text wall, so I can skim the reply's shape and spot the parts needing my answer without reading linearly.

## Purpose

Every agent reply paints as one unformatted wrapped label with literal asterisks and hashes in Main Chat, card tabs, and live streaming, which the operator reports as hard to read. This ticket extracts the spec viewer's private light-palette pulldown-cmark 0.13 event fold into a shared dark-theme Markdown painter driven by theme.rs colors and routes agent reply prose through it at chat_pane::paint_message (agent branch), the task_chat transcript loop, and paint_progress chunks (painted progressively, degrading harmlessly), while user bubbles, Thinking collapsibles, and monospace Tool output keep their exact plain regime and the spec viewer's light path stays behavior-identical; envelope shielding in message_text.rs is preserved so no JSON ever reaches a chat render.

## Specification references

Source: [Approved specification](specification.md)

## Implementation context

Verified at root d881038 (/mnt/DevProj/Packet). (1) src/ui/spec_viewer.rs owns a private pulldown-cmark 0.13 fold (Span/State, ~lines 66-440): heading/paragraph/blockquote/fenced-code/strong/emphasis/strikethrough/list/table events; sizes body 15.0, H1 30, H2 22, H3 18, H4+ 13.5, code 12.0, table 13.5; hardcoded light consts INK (31,41,55), MUTED (85,98,116), code-tag (23,83,151); tables via ui.columns, code via horizontal ScrollArea; 'Options::ENABLE_TABLES|ENABLE_STRIKETHROUGH'; bold forces INK; strike stroke is MUTED. Two shape-asserting tests pin the paper look. (2) chat_pane::paint_message (188-227) paints ALL roles as one wrapped RichText label of m.text.trim() at 15.0/lh23/theme::TEXT; user gets PANEL_ALT bubble + 32px indent; agent/System transparent. Feeds Main Chat (layout.rs:474) and card tabs (layout.rs:445), where layout.rs:432 pre-swaps task messages' text with message_text::readable. (3) chat_pane::paint_progress (234ff) paints LiveProgress: posts kind 'thinking' (collapsing dim), 'tool' (collapsing monospace), else plain label — else posts are kind 'text' pre-projected by harness/live_preview.rs project(), which separates the JSON envelope and withholds a partial opening fence, so streamed chunks are prose-only even mid-turn; the posts-empty fallback also paints progress.response (same projection). (4) Provenance: accepted main turns persist normalized.assistant_message (envelope already stripped, root.rs ~743); rejected turns persist raw final_text (~795); PM proactive updates persist raw text prompted 'Return conversational plain text, not JSON' (manager.rs); task tabs persist raw and shield at paint time (task_chat.rs:100). (5) Executed baselines at d881038: cargo test --offline = 290 lib + 2 integration, 0 failed; cargo clippy --all-targets --offline (rust 1.98.0) = 110 pre-existing warning lines package-wide, 7 in src/ui (toast.rs:87, task_chat.rs:320/350/405, spec_viewer.rs:277/400, layout.rs:912) — so the enforceable bar is zero NEW warnings versus baseline, not a tree-wide warning-free clippy; the two spec_viewer warnings (:277 unwrap_after_is_some, :400 let_return) sit in code this ticket moves. (6) Boundaries: no new crates (pulldown-cmark 0.13.4 already declared in Cargo.toml); diff confined to src/ui/** (no src/core in this ticket — that is story 3); user-entered text is 'never Markdown-rendered from user input' (chatlog.rs doc) and must stay plain.

Feature ID: CHG-003
Repository: root

## Technical design and contracts

- NEW FILE src/ui/markdown.rs exposing `pub struct Style { ink, muted, code_tag: egui::Color32; body, h1, h2, h3, h_other, code, table: f32 }`, `pub const PAPER: Style` pinning the spec viewer's exact current literals (ink 31,41,55; muted 85,98,116; code_tag 23,83,151; sizes 15/30/22/18/13.5/12/13.5), `pub const CHAT: Style` (ink theme::TEXT, muted theme::TEXT_DIM, code_tag theme::TEXT_DIM; sizes 15/20/17/15.5/15/12/13 — body 15 keeps today's 15px chat density), and `pub fn paint(ui: &mut egui::Ui, md: &str, style: Style)`.
- paint() ports the spec_viewer fold VERBATIM in algorithm (Span/State: strong/emph/strike counters, list-stack markers '• '/'N. ', blockquote '> ' muted prefix span, table cell buffering drawn via ui.columns, fenced code via horizontal ScrollArea with per-language label, Rule to ui.separator(), SoftBreak '\n'/HardBreak '\n\n', ENABLE_TABLES|ENABLE_STRIKETHROUGH), now threaded by Style: base span color falls back to style.ink, bold forcing uses style.ink with the existing extra-letter-spacing mechanism, strike stroke color is style.muted, heading sizes resolve from the style tuple, code/lang/table font sizes come from the style, wrap max_width stays ui.available_width() — behavior for PAPER must be byte-identical to today's paper.
- Wiring in chat_pane::paint_message: ONLY the ChatRole::Agent branch (user bubble and System 'Update'-tagged notices stay byte-identical) computes `let shielded = crate::ui::message_text::readable(m);` then calls `markdown::paint(ui, &shielded, markdown::CHAT)` inside the existing transparent rounded frame and existing constrained width — one code path serves Main Chat and card tabs (tab messages arrive pre-shielded by layout.rs:432 and readable() is idempotent on already-shielded prose, which the test matrix pins).
- paint_progress: the posts-loop `else` arm (kind 'text') and the fallback `progress.response` label switch to `markdown::paint(ui, text.trim(), markdown::CHAT)`; Thinking collapsible content, Tool collapsible monospace output, and the activity line keep their exact current regimes. Chunks paint progressively: pulldown-cmark parses every in-flight prefix without panicking, and live_preview::project already withholds the envelope and partial opening fences, so no JSON reaches the stream — this invariant is preserved by trusting that harness projection, not reimplementing shielding in the UI.
- task_chat::transcript(): only the agent-role branch switches from the plain label to `markdown::paint(ui, readable.as_ref(), markdown::CHAT)` (readable already computed at line 100); user and System branches and the monospace 'Response details' collapsible are untouched.
- Registration and refactor: add `pub mod markdown;` to src/ui.rs; spec_viewer::render_markdown becomes `markdown::paint(ui, md, markdown::PAPER)`; delete INK/MUTED/Span/State/prefix/heading_size (~330 lines); the paper frame, NO_CONTENT_HINT, min-width and centering stay, and the two incumbent shape tests pass with only their INK references renamed to markdown::PAPER.ink.
- Quality gate: the moved code fixes the two incumbent clippy findings that live in it (spec_viewer.rs:277 `unwrap` after `is_some` becomes `match self.code_buf.take()`-style consumption; :400 let_return vanishes by plain if-block); no other warning may be ADDED versus the 110-line baseline captured at task start, and no warning may name src/ui/markdown.rs; no Cargo.toml delta.

## Approved scope mapping

- Scope 1: Shared dark-theme Markdown painter (factor/expose the spec viewer's pulldown-cmark→egui event fold) applied to agent replies: final transcript (Main Chat + card tabs) and live-streaming reply text; user bubbles and Thinking/Tool diagnostic collapsibles remain plain.
- Success criterion 1: Agent replies in Main Chat, card tabs, and live streaming render bold/lists/headings/code with no visible raw markers, using only the existing pulldown-cmark dependency (no new crates).
- Success criterion 5: Warning-free clippy/build, full regression suite green, no new dependencies; diff confined to src/ui/** plus two prose-only prompt paragraphs in src/core.

## Dependencies

None. This task can start independently.

## Affected files and components

- PROPOSED src/ui/markdown.rs: new shared Markdown painter — Style struct, PAPER/CHAT constants, paint() carrying the factored cmark->egui event fold, plus in-file egui shape-assertion tests
- EXISTING src/ui.rs: one-line module registration `pub mod markdown;` among the other view modules
- EXISTING src/ui/spec_viewer.rs: delete the private fold (Span/State/prefix/heading_size/INK/MUTED consts), delegate the inner render to markdown::paint(_, _, PAPER), retarget the two incumbent tests' INK references to PAPER.ink — visible paper appearance preserved
- EXISTING src/ui/chat_pane.rs: paint_message agent branch gains readable()-shielded markdown::paint; paint_progress 'text' posts and progress.response fallback switch to the painter; user/System/thinking/tool arms untouched
- EXISTING src/ui/task_chat.rs: transcript() agent branch switches to markdown::paint on the already-computed readable text; user/System plain labels and 'Response details' collapsible untouched
- EXISTING Cargo.toml: explicitly NOT modified — pulldown-cmark 0.13.4 is already a declared dependency; this ticket adds no crates

## Implementation steps

1. Capture the pre-work baselines from /mnt/DevProj/Packet: `cargo clippy --all-targets --offline 2>&1 | grep '^warning' | sort > /tmp/clippy_base.txt` (expected 110 lines) and `cargo test --offline` (expected 290 lib + 2 integration passing), so the final state compares mechanically against d881038.
2. Create src/ui/markdown.rs by porting spec_viewer.rs's Span/State fold verbatim with Style threaded through Span::fmt, heading-size resolution, code-language label color, and table-cell size; define PAPER pinning today's exact RGB triples and sizes (the byte-identity anchor) and CHAT themed off theme.rs (15/20/17/15.5/15/12/13); restructure the two warned patterns while moving so the new file is warning-clean; add the in-file shape tests described in test_plan.
3. Wire chat_pane.rs: in paint_message replace the agent body label with `markdown::paint(ui, &message_text::readable(m), markdown::CHAT)` inside the existing frame/width (roles != Agent untouched); in paint_progress convert the else-post arm and the progress.response fallback to the painter; keep thinking/tool/activity arms byte-identical.
4. Wire task_chat.rs transcript(): agent branch calls `markdown::paint(ui, readable.as_ref(), markdown::CHAT)` in place of the plain label; rebuild and run the full suite immediately to catch any coupling before continuing.
5. Refactor spec_viewer.rs: render_markdown delegates to markdown::paint(ui, md, markdown::PAPER); delete the now-private fold and constants (~330 lines removed); rename INK references in the two incumbent tests to markdown::PAPER.ink WITHOUT altering their assertions — passing them unchanged in substance proves the light path stayed identical.
6. Add the chat_pane wiring test (public paint entry, synthetic Agent/User/System ChatMessages with Markdown-bodied text) asserting styled marker-free output for Agent, raw-marker-preserving plain output for User/System, so the role gating is regression-proofed.
7. Close the gates: `cargo test --offline` fully green (>=292 passing, incl. new tests); `cargo clippy --all-targets --offline` warning-line diff vs /tmp/clippy_base.txt shows ZERO additions, no line naming src/ui/markdown.rs, and a net decrease of exactly the fixed-moved-code warnings; `cargo check --all-targets --offline` finishes without error; verify the diff touches only the five src/ui paths above.

## Acceptance criteria

- Given a Main Chat containing a stored agent reply with **emphasis**, a numbered list, a ### heading and a ```rust fence, when the pane paints, then no galley text contains '**', '###' or backtick-fence markers, bold spans carry a distinct TextFormat from sibling plain spans, and the heading section renders at CHAT.h1 size.
- Given a card-tab conversation whose persisted raw text is a protocol envelope {"assistant_message":"Hi **you**","schema_version":1,...}, when the tab paints, then the reply renders the readable prose with styled emphasis and no 'assistant_message' key or braces appear in rendered text (layout.rs pre-shield plus paint_message re-shield compose safely).
- Given a live turn delivering growing prefixes of a reply containing an unclosed '**word' and a half-typed fence, when paint_progress repaints on each progress update, then no panic or error occurs, the partial fragment degrades to harmless literal glyphs, and the completed text renders fully formatted.
- Given a user bubble and a Thinking collapsible whose text contains literal Markdown markers, when painted, then both remain plain text exactly as today (markers visibly present, bubble fill/indent and collapsible monospace regimes unchanged) — conversion is agent-prose-only.
- Given the spec viewer's incumbent test documents, when rendered after the refactor, then the white paper frame, (31,41,55)-colored body sections and centering/bounds assertions pass unchanged, proving the light palette path is behavior-identical.
- Given degenerate reply bodies — empty string, whitespace only, a lone '---', a single word, and an unclosed code fence — when painted in any of the three wired sites, then the painter emits zero-or-more shapes without panicking and leaves surrounding chrome (frames, composer, buttons) intact.

## Test plan

1. Fixture-matrix test in markdown.rs: build a document exercising H1-H4, bold/italic/strikethrough, inline code, nested unordered+ordered lists, blockquote, horizontal rule, a 3-column table, and a ```rust fence; drive ctx.run_ui and walk output.shapes asserting per construct: no galley text contains '**', '~~', '###' or triple backticks; the H1 section's font size equals CHAT.h1 with ink theme::TEXT; a struck span's format.strikethrough stroke color equals theme::TEXT_DIM; code spans/lines use FontFamily::Monospace; the rule yields a Shape::Line; and PARSER-LEVEL PIN: PAPER's ink/muted/code_tag equal from_rgb(31,41,55)/(85,98,116)/(23,83,151) with sizes 15/30/22/18/13.5/12/13.5.
2. Streaming prefix sweep: over the fixture (fence+bold+table), loop every prefix length from 1 to full in varied strides; assert no panic on any iteration, the complete input yields at least one Text shape, and one chosen mid-bold prefix demonstrably renders a galley containing a literal '*' (locking the documented degrade-not-drop contract).
3. chat_pane wiring test: seed Agent, User and System ChatMessages sharing one Markdown body; call public chat_pane::paint with a scratch draft; assert the Agent galley is marker-free with a styled bold section, while the User and System galleys still contain the raw markers (negative controls locking the role gate); repeat the Agent case through paint_task-shaped arguments to cover the card-tab entry.
4. Spec paper regression: keep the two incumbent spec_viewer tests semantically untouched (only INK renamed to PAPER.ink) and assert they still discover the WHITE paper rectangle, (31,41,55) body sections and the centering bounds — the mechanical proof the light path is behavior-identical after the factor-out.
5. Gate run: `cargo test --offline` reports 0 failures across lib plus both integration suites (total >= 292 incl. new tests); `cargo clippy --all-targets --offline 2>&1 | grep '^warning' | sort` diffs against /tmp/clippy_base.txt with ZERO new lines, none naming src/ui/markdown.rs, and deletions only where the moved spec_viewer code's two findings were fixed.

## Verification commands and expected evidence

1. Working dir /mnt/DevProj/Packet: `cargo check --all-targets --offline` — expected: 'Finished `dev` profile ... ' with no errors (verified at d881038, completes in ~0.2s on warm cache).
2. Working dir /mnt/DevProj/Packet: `cargo test --offline` — expected: 'test result: ok. 290 passed; 0 failed' for the lib plus '1 passed' each for tests/multi_repository_feature.rs and tests/task_workflow.rs (verified at d881038; after this ticket expect >= 292 total passed, 0 failed).
3. Working dir /mnt/DevProj/Packet: `cargo clippy --all-targets --offline 2>&1 | grep -c '^warning'` — expected: 110 warning lines at the pre-change baseline (verified at d881038 on rust 1.98.0); after this ticket the count must be lower or equal with zero lines referring to src/ui/markdown.rs.

## Edge cases and failure handling

- Half-finished emphasis mid-stream: the model streams '**Bold' before the closing arrives; pulldown-cmark renders the literal stars for those ticks and styles retroactively when the close lands — the prefix-sweep test asserts only non-panic during streaming, never marker absence.
- Partial opening fence: live_preview::project withholds unterminated opening backticks (trim_end_matches('`')) before any chunk reaches paint_progress, so a half-typed '```' cannot open a spurious giant code block; the UI painter trusts that harness projection and must not re-implement or defeat it.
- Double shielding on card tabs: layout.rs:432 already swapped the stored text with readable() output before paint_message shields again; the test matrix must prove a typical shielded reply round-trips unchanged (second application returns identical or equivalently re-extracted text), guarding against compounded trimming or accidental re-mask.
- Rejected-turn raw final_text stored to Main Chat: previously the raw (possibly braced) JSON painted verbatim; with paint_message now shielding, readable() yields the friendlier 'unreadable reply' note for broken envelopes and unwraps intact embedded envelopes — deliberate alignment with 'no JSON ever reaches a chat render' while plain undecorated legacy replies pass readable() unchanged and thus paint identically to today.
- Wide tables and long code lines in the narrow chat pane: cells wrap at ui.available_width() and code scrolls horizontally exactly as in the paper, and the agent body remains constrained by paint_message's existing set_width, so content neither escapes the bubble nor reshapes the pane.

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

Pure presentation change: chat.jsonl and task-chat persistence formats, LiveProgress serialization, the harness, and all src/core logic are untouched, so there is no migration, seeding, or schema-version concern. Rollback is reverting the single commit; nothing residual survives because no state was created and readable() is stateless. Every stored legacy reply re-renders under the new painter automatically at next launch. The only forward-visible shifts — rejected raw-envelope Main Chat messages gaining the friendly unreadable-reply note, and agent prose generally becoming formatted — are both desired outcomes of this feature.

## Definition of done

- The diff touches only src/ui/markdown.rs (new), src/ui.rs, src/ui/spec_viewer.rs, src/ui/chat_pane.rs and src/ui/task_chat.rs — no Cargo.toml, src/core, src/harness or tests/ path is modified.
- `cargo test --offline` is fully green including the new markdown/chat_pane shape tests and the spec paper tests passing with assertions substantively unchanged.
- Clippy baseline comparison shows zero added warnings and none in src/ui/markdown.rs, and `cargo check --all-targets --offline` completes without errors.
- Marker-absence shape assertions pass on all three wired surfaces (Main Chat agent bubble, card transcript agent entry, streamed prefix sweep) including the degenerate inputs: empty, lone '---', and unclosed fence.

## Instructions for the implementing model

Read this story and its linked dependencies before editing. Confirm the named existing files and interfaces; proposed files are explicitly labeled. Implement only this task's scope, preserving the constraints above. Run the verification commands and record actual results, changed files, and any remaining limitations. Do not mark complete on a build alone when acceptance criteria require runtime interaction. If an assumption contradicts the repository or approved specification, report the discrepancy rather than silently inventing a requirement.
