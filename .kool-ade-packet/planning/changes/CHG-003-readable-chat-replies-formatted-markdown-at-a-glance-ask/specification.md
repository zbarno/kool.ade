# CHG-003: Readable Chat Replies — Formatted Markdown, At-a-Glance Asks, and Quick Option Chips
<!-- packet-artifact-id:v1 {"uid":"2026356e-2bfa-40fa-bef7-de68b792c62a","displayId":"CHG-003","title":"Readable Chat Replies — Formatted Markdown, At-a-Glance Asks, and Quick Option Chips"} -->

**Status:** Implementing
**Directed by:** Operator statements in the Main Chat interview: the formatting complaint; the trailing 'ask/recommendation/point' list ('almost like a TL;DR, but not explicitly called out that way'); tappable controls for explicit options — with an explicit refusal to limit replies to listable forms.
**Bound:** Chat display plus prose-only prompt-contract edits. The §12 MVP exit bar (D-21) is untouched; D-29 approval ordering is untouched.

## Intent

Planner replies arrive as one plain wrapped text label in chat (Main Chat, per-card tabs, live streaming): a wall of text where raw Markdown markers print as literal asterisks and hashes, and whatever the operator is being asked for sits buried in prose (operator report: harder to read, harder to see what the agent needs). Three treatments were agreed with the operator:

1. **Format** — render agent replies as structured Markdown (emphasis, lists, headings, code) via the in-tree pulldown-cmark 0.13 stack the spec viewer already exercises; no new crates.
2. **At-a-glance** — when a reply awaits something from the operator, end it with a compact, UNLABELED digest: a hairline rule plus ≤5 short bullet lines ordered ask → recommendation → pointer. It is deliberately not named anything (explicitly 'not a TL;DR label'); it exists only when something is actually awaited.
3. **Quick options** — when a digest enumerates discrete choices (Yes / No / Option 1 / lettered), each choice becomes a tappable chip that INSERTS the option text into the composer draft. Chips never auto-send and never obligate: every reply remains fully answerable by free typing, per the operator's explicit non-limitation requirement.

Intended user: the seated operator (Zachary Barno, per D-23) reading planner replies on the Linux desktop. Not owed: new telemetry, other surfaces, user-bubble rendering, auto-send.

## Current Behavior

Code-grounded, read-only audit of master before CHG-003:

- Agent replies paint as a single plain wrapped label everywhere: `chat_pane::paint_message` (src/ui/chat_pane.rs ~210–227) draws every transcript bubble as `egui::Label::new(RichText::new(m.text.trim())).wrap()` — no Markdown transform; user bubbles differ only by a PANEL_ALT pill. The streaming path (`paint_progress`, chat_pane.rs ~236–310) likewise paints post/thoughts/activity/response-chunk text plainly.
- Main-chat history is envelope-sanitized before display at `src/ui/layout.rs:432` (`readable.text = message_text::readable(...)`), so transcripts show the extracted assistant_message prose, never the JSON envelope; `message_text.rs` additionally shields broken/truncated envelopes with fixed guard sentences.
- Card chats ALREADY lift the trailing ask: `task_chat::split_reply` (src/ui/task_chat.rs:9–33) splits on 'Your next step:' (tolerating `**bold**` dressing), with a legacy fallback that lifts the final line ending in '?' (≤280 chars) from older undecorated replies; the lifted block renders under 'Your next step' / 'Your answer needed' headings (task_chat.rs ~235–330) beside Send answer / Assign ownership / Review decision buttons. That convention is TASK-CONVERSATION-MODE ONLY, enforced at `src/core/turn.rs:264` (end with a line beginning exactly 'Your next step:', or end 'No reply needed.').
- The Main Chat prompt (src/core/prompt.rs:67+, OUTPUT STYLE / RESPONSE CONTRACT) carries NO trailing-ask convention — main-chat asks are free prose.
- The Markdown engine already exists in-tree and is exercised: `spec_viewer::render_markdown` (src/ui/spec_viewer.rs:48) folds pulldown-cmark 0.13.4 (Cargo.toml:21) events into styled egui LayoutJobs (bold/mono/italic/strikethrough; tables via Grid with Options::ENABLE_TABLES + ENABLE_STRIKETHROUGH). It is private and hardwired to the light palette (INK 31,41,55 / MUTED 85,98,116), unreachable from chat today.
- The envelope extractor takes the LAST fenced ```json block (src/harness/pi_extract.rs:4–9, test-locked incl. story-embedded fences), so any prose convention placed before the final fence cannot disturb extraction.
- Documented stance: user-entered message text is 'Plain text (never Markdown-rendered from user input)' (src/domain/chatlog.rs:28).
- Composer: one multiline draft per chat (chat_pane TextEdit; send via button or Cmd/Ctrl+Enter; task_chat adds Send answer / Stop reply). No quick-fill or insert affordance exists anywhere in chat UI today.

## Desired Behavior

**R1 — Markdown for agent replies, all chat surfaces.**

- Final agent replies render through a shared DARK-THEME Markdown painter fed by the same pulldown-cmark event fold the spec viewer uses: emphasis, headings (sized down for chat density), bullets/ordered lists, inline code, fenced code blocks, links; tables render when present (engine already enables them), degrading harmlessly elsewhere. Raw markers vanish after transform.
- Applied to: Main Chat transcript agent bubbles (`paint_message`, agent-role branch), per-card tab transcripts (task_chat message loop), and live-streaming reply text (`paint_progress` response/post chunks, painted progressively).
- User bubbles, Thinking collapsibles, and Tool-output collapsibles REMAIN plain (tool output stays monospace) — diagnostics, not replies.

**R2 — the unlabeled at-a-glance tail.**

- The reply-shaping contract (main mode AND task mode) becomes: when a reply awaits operator input, the prose ends with a compact digest — a horizontal rule line followed by ≤5 short bullet lines in the order ask (what I need from you) → recommendation (my provisional lean) → pointer (evidence/where it lives). No label, no ornament. When nothing is awaited, the reply keeps the existing 'No reply needed.' closeout and carries no digest.
- Detection (UI-side, from message prose only): a final '---' line followed by ≤5 short lines, at least one bulleted ⇒ tail. LEGACY: any reply whose tail line(s) start 'Your next step:' still lift exactly as today on cards, and ALSO lift on Main Chat (parity gain — this is the existing card block reaching its sibling surface). No marker ⇒ plain body, no tail chrome, no chips, no error. 'No reply needed.' ⇒ no tail chrome.
- Presentation: the tail renders as a visually distinct block (hairline top rule, slight indent/backdrop) directly under the transformed body on BOTH Main Chat and card surfaces. The card's existing heading + Send answer machinery consumes the generalized digest; a one-line digest maps 1:1 to today's card behavior.

**R3 — quick option chips, offered only when options exist.**

- When digest bullets enumerate discrete choices — leading-token grammar, case-insensitive: 'Yes', 'No', 'Option N', or a single letter followed by ')' or '.' — each recognized option, when the count is 2–6, renders as a compact clickable chip row beneath the digest, on both surfaces.
- Chip tap INSERTS the option text into that chat's composer draft (append, caret at end, input focused). It does NOT send. The composer stays fully interactive at all times; typing supersedes/coexists freely. Fewer than 2 or more than 6 recognized options, or no digest ⇒ no chips, prose only.
- Chips display the option token plus up to ~40 characters of the descriptor; hover reveals the full option text.

**R4 — no leakage, no regression.**

- Streaming and finished replies never expose the JSON envelope or implementation fields (today's message_text shielding preserved); broken envelopes keep their existing 'unreadable reply' guard sentence.
- `pi_extract` is UNTOUCHED; an invariant test locks that last-fence extraction is byte-identical on a digest-and-chips reply versus the same reply without the digest.

## Scope

In:

1. Dark-theme Markdown painter sharing the spec viewer's pulldown-cmark event fold (factor `render_markdown` to callable-from-chat with a dark palette, or a shared module beside it); wired into agent replies in Main Chat, card tabs, and streaming. Spec viewer's light path remains behavior-identical.
2. Tail digest: prose edits to the reply contract (main + task mode), a generalized detector replacing `task_chat::split_reply` (legacy 'Your next step:' and final-'?' fallbacks preserved), and the lifted tail block brought to Main Chat at card parity.
3. Chips: option-grammar detector over digest bullets, chip-row widget, tap-to-insert wiring into both composers; freeform never gated.
4. In-file unit tests (painter smoke; detector matrix; insert path; extractor invariant); D-34 no-new-warnings bar (no new clippy warnings versus the recorded pre-work baseline, on the pinned Rust 1.98 toolchain) plus full regression; no Cargo.toml change.

Out:

1. Markdown rendering of user-entered messages (chatlog.rs:28 stance stands; one-word re-open later).
2. Auto-send from chips, chip keyboard navigation, animation, selectable themes/fonts.
3. Spec viewer or other surfaces (already Markdown), and Markdown capabilities beyond the core engine set (headings/emphasis/lists/code/links/tables as the engine already yields).
4. Envelope schema, routing/veto, streaming-timeout/cancel semantics, persistence formats; ZERO src/harness changes.

## Affected Product Areas

Modules marked for change are tagged with their logical document IDs; updates land at reconciliation (FR-15 / D-28 discipline — no new product MUST for unmerged behavior).

- **Module 05 (Functional Requirements)** — `product:05-functional-requirements`: the chat-display duties gain 'structured Markdown rendering + lifted at-a-glance ask + optional chips' at reconciliation; the task-conversation 'Your next step' duty is GENERALIZED, not replaced.
- **Module 10 (Decisions Log)** — `product:10-decisions`: operator-directive entries (unlabeled digest shape, chips-when-options, user-bubble plain-text retention) recorded at approval/reconciliation with the next free D-numbers; interim authority sits in this document and the interview record.
- **Module 13 (Source Map)** — `product:13-source-map`: current-authority row and chat rows gain this feature document at reconciliation.
- **Code:** `src/ui/message_text.rs` or new sibling `src/ui/reply_tail.rs` (detector), `src/ui/chat_pane.rs` (bubble + progress painter wiring), `src/ui/task_chat.rs` (split_reply generalization, chip row, draft insert), `src/ui/spec_viewer.rs` (factor/share; light path unchanged), `src/ui/theme.rs` (tail-block + chip tints). Prose-only: `src/core/prompt.rs`, `src/core/turn.rs`. Tests in-file.

## Requirements

- **REQ-R1-1 (MUST).** Agent replies in Main Chat, card tabs, and streaming render emphasis/lists/headings/code through the in-tree pulldown-cmark path; no raw Markdown markers remain visible in reply areas after transform; user bubbles and Thinking/Tool collapsibles retain their current plain/monospace regimes.
- **REQ-R2-1 (MUST).** A reply awaiting operator input ends with the unlabeled digest (rule + ≤5 bullets: ask, recommendation, pointer) rendered as a visually distinct block on BOTH Main Chat and card surfaces; replies awaiting nothing carry no digest, respecting 'No reply needed.'
- **REQ-R2-2 (MUST).** Legacy stored replies keep today's behavior: 'Your next step:' lines lift on both surfaces (cards unchanged, Main Chat newly at parity); the final-'?' fallback and no-marker passthrough remain intact.
- **REQ-R3-1 (MUST).** Discrete-option digests (2–6 bullets with Yes/No/Option-N/letter leading tokens) render a tappable chip row, one chip per option; tapping inserts that option's text into the chat's composer draft WITHOUT sending; composer typing remains available at all times; no chips appear when the grammar does not hold.
- **REQ-R4-1 (MUST).** Envelope hygiene: no envelope or implementation field reaches any chat render; broken envelopes keep the existing guard text; last-fence extraction is provably unaffected by digest/chip replies (invariant test).
- **REQ-ALL-1 (MUST).** Meets the NFR-8 bar as fixed by D-34: full regression suite green (incumbent tests plus new), and no NEW clippy warnings versus the recorded pre-work baseline at the commit under verification, evaluated on the pinned toolchain (Rust 1.98) — the literal globally-zero-warnings reading is superseded for this gate; no new Cargo.toml dependencies; diff confined to `src/ui/**` plus the two prose-only prompt paragraphs in `src/core/prompt.rs` and `src/core/turn.rs`.

## Decisions and Assumptions

- **A1 (operator statement, locked):** the tail digest is UNLABELED ('almost like a TL;DR, but not explicitly called out that way'); options are offered as UI controls only when the reply actually enumerates them; replies are never limited to listable forms — freeform typing always remains the full path.
- **A2 (provisional, board-reviewable, planner-chosen):** digest grammar = final '---' rule + ≤5 '- ' bullets in ask → recommendation → pointer order; the 'Your next step:' marker is honored permanently for stored-history compatibility; 'No reply needed.' suppresses the digest. Overridable by a fresh word without reopening A1.
- **A3 (provisional, board-reviewable, planner-chosen):** chip grammar (Yes / No / Option N / single letter, case-insensitive, 2–6 range) and tap = INSERT INTO DRAFT (append, focus, caret at end) with NO auto-send. Considered and rejected alternative: auto-submit on tap — rejected because sending is irreversible and the composer must remain the single send authority.
- **A4 (provisional):** user-entered text stays plain (chatlog.rs:28 stance); symmetric user-Markdown deferred.
- **A5 (provisional):** streaming reply text is Markdown-painted progressively (partial documents degrade harmlessly); Thinking/Tool collapsibles stay plain/monospace.
- **A6 (scope concession, recorded):** this feature deliberately touches `src/core` as PROSE-ONLY prompt-constant edits (two paragraphs) — the first non-`src/ui` touch since the MVP — because the digest/chips depend on model output shape. No logic, no semantics, no harness changes; contained by REQ-ALL-1's diff-scope assertion.
- **CLR-021 / D-34 (resolved; operator ruling 'yes' in the CLR-021 conversation):** this feature's 'warning-free' wording in REQ-ALL-1 and Acceptance Criterion 6 is hereby read per D-34 — no NEW clippy warnings versus the recorded baseline at the commit under verification, on the pinned Rust 1.98 toolchain (initial global baseline: 110 warnings at 3ba5aa2). Story text in this batch that asserts a globally zero-warning clippy run (stories 002 and 003) is SUPERSEDED where it conflicts; story 004's pre-work baseline-diff method (sorted warning listing diffed against the pre-work capture: zero added lines, no warning naming a symbol this feature adds) is the binding operational form of the gate. Cleaning the pre-existing 110 warnings is NOT owed by this batch (module 11 tracks it as a standalone maintenance sweep). Because the document changes after approval registration, D-29's ledger-snapshot equality may require re-running the in-app Approve action on this document before further queue progression; the effective bar is unchanged for the in-flight batch, which already operates on the pre-work baseline.
- **Sequencing note:** D-29 approval order holds — the CHG-002 verification gate concludes first; CHG-003 enters the approval pipeline only via the in-app Approve action on this promoted document (the ledger entry, not prose, releases the generation gate).
- **Kickoff (operator directive, recorded in Main Chat):** the operator directed implementation to begin; the approved five-story batch under `.kool-ade-packet/planning/tasks/readable-chat-replies-with-at-a-glance-asks-and-quick-op` dispatches from story 001 via the board/queue controls. The in-app **Approve feature for implementation** action on this document registers the CHG-003 approval-ledger entry (ledger precedent: CHG-001 and CHG-002 each gained theirs by that action). The prerequisite CHG-002 verification gate is closed by CHG-002's reconciliation.

## Acceptance Criteria

Each observable in the built app against the seeded fixture repository:

1. Seeded reply corpus (bold/italic prose, nested bullets, an h3, inline + fenced code, a link) renders formatted in Main Chat, a card tab, and mid-stream; zero leftover '*'/'#' markers in reply areas; user bubbles, Thinking, and Tool-output styling unchanged.
2. A scripted reply ending in '---' + ask / recommendation / pointer bullets shows the hairline-separated distinct block, and when the bullets are options ('Yes — …', 'No — …', 'Option 2 — …') three chips render beneath; tapping a chip places exactly that option text into the draft, unfocused-send impossible, caret at end; manual typing and Cmd/Ctrl+Enter still send normally.
3. A reply with no tail rule renders as plain body with zero chip row; 'No reply needed.' replies show no tail chrome and no card ask.
4. Regression: a pre-feature 'Your next step:' reply lifts on Main Chat (new parity) and on cards (behavior unchanged); the final-'?' fallback holds; the existing split_reply test matrix passes under the generalized detector.
5. Invariant: pi_extract's suite is unmodified and green, plus a new test proving digest replies extract the envelope byte-identically to the same reply minus its digest.
6. Gate: meets the NFR-8 bar as fixed by D-34 — full suite green, and no NEW clippy warnings versus the recorded pre-work baseline at the commit under verification on the pinned Rust 1.98 toolchain (superseding the formerly literal 'reports no warnings' reading), no Cargo.toml delta, diff confined per REQ-ALL-1.
