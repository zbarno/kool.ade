//! Shared reply-tail classifier. Operates on PRE-ENVELOPE reply prose only
//! (what `message_text::readable` hands out) and NEVER rewrites message
//! text: `ReplyTail::body` is a trimmed view of the supplied input and the
//! lifted ask/bullets are verbatim fragments of it. This module also hosts
//! the shared tail-block painter (`paint_open_ask`) and the per-pane
//! selector (`open_ask_index`) deciding which stored agent message still
//! owes the operator an answer.
//!
//! Classification order is fixed and legacy-first:
//! `Your next step:` marker → `No reply needed.` closeout → the new
//! unlabeled digest → final-`?` fallback → plain. A malformed digest tail
//! degrades to the fallback/plain paths; a non-conforming tail is never
//! partially lifted. The classifier is pure and linear over the input, so
//! in-progress streaming prefixes classify deterministically.
use crate::domain::chatlog::{ChatMessage, ChatRole};
use crate::ui::theme;


/// The shape detected at the end of a reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TailKind {
    /// Fresh digest: final `---` rule line followed by 1..=`MAX_DIGEST_BULLETS`
    /// short bullet lines.
    Digest,
    /// Legacy `Your next step:` marker (tolerates the `**bold**` dressing).
    NextStep,
    /// Legacy undecorated fallback: the last non-empty line ends in `?`
    /// within the 280-char cap (measured on the un-trimmed line).
    FinalQuestion,
    /// Closed with `No reply needed.`; no digest rides along.
    NoReply,
    /// No recognisable tail marker.
    #[default]
    Plain,
}

impl TailKind {
    /// Whether the tail awaits something from the operator. `NoReply` and
    /// `Plain` never do.
    pub const fn asks_input(self) -> bool {
        matches!(self, Self::Digest | Self::NextStep | Self::FinalQuestion)
    }
}

/// The classified reply tail with its lifted payloads extracted.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ReplyTail {
    /// Detected shape.
    pub kind: TailKind,
    /// Reply body with the tail excluded (Plain: the whole prose, trimmed).
    pub body: String,
    /// The operator-facing request, when one was lifted.
    pub ask: Option<String>,
    /// True only for the `No reply needed.` closeout.
    pub no_reply: bool,
    /// Digest bullets in source order; empty for every non-Digest kind.
    pub bullets: Vec<String>,
}

/// Most bullet lines a digest may carry; beyond that the whole digest degrades.
pub const MAX_DIGEST_BULLETS: usize = 5;
const MAX_BULLET_CHARS: usize = 240;
const MAX_QUESTION_CHARS: usize = 280;

/// Splits raw pre-envelope reply prose into exactly one of the five reply
/// shapes. See the module docs for the fixed classification order; this
/// function never mutates or reflows `prose`.
pub fn parse_reply_tail(prose: &str) -> ReplyTail {
    let flattened = prose.replace("**Your next step:**", "Your next step:");
    if let Some((body, ask)) = flattened.split_once("Your next step:") {
        // Case-sensitive, first occurrence wins; an empty post-marker
        // contributes no question.
        return ReplyTail {
            kind: TailKind::NextStep,
            body: body.trim().to_string(),
            ask: Some(ask.trim().to_string()).filter(|ask| !ask.is_empty()),
            ..Default::default()
        };
    }
    if prose.trim_end().ends_with("No reply needed.") {
        return ReplyTail {
            kind: TailKind::NoReply,
            body: prose
                .trim_end()
                .trim_end_matches("No reply needed.")
                .trim()
                .to_string(),
            no_reply: true,
            ..Default::default()
        };
    }
    if let Some(tail) = digest_tail(prose) {
        return tail;
    }
    // Older undecorated replies: surface an explicit final question verbatim.
    let body = prose.trim();
    if let Some(ask) = body
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .filter(|line| line.trim().ends_with('?') && line.chars().count() <= MAX_QUESTION_CHARS)
        .map(|line| line.trim().to_string())
    {
        let body = if ask == body { String::new() } else { body.to_string() };
        return ReplyTail {
            kind: TailKind::FinalQuestion,
            body,
            ask: Some(ask),
            ..Default::default()
        };
    }
    ReplyTail {
        kind: TailKind::Plain,
        body: body.to_string(),
        ..Default::default()
    }
}

/// The index of the last stored agent reply whose outstanding ask is still
/// open, or `None` when no reply is owed to the operator.
///
/// Rules, locked by the in-file selector table:
///   * walk from the END (`rposition`) for the last `ChatRole::Agent` message;
///   * ANY `ChatRole::User` message after it answers the ask, retiring the
///     lift (a user answer stays an answer);
///   * a `ChatRole::System` notice after it does NOT clear the lift — a
///     proactive notice leaves the ask genuinely outstanding;
///   * the candidate tail must itself classify as input-seeking via
///     [`TailKind::asks_input`], so `No reply needed.` closes settle quietly.
///
/// Pure and stateless, allocating only for the single candidate message;
/// callers evaluate it once per frame per pane, mirroring how the card's
/// `split_reply` is recomputed today.
pub fn open_ask_index(messages: &[ChatMessage]) -> Option<usize> {
    let index = messages.iter().rposition(|m| m.role == ChatRole::Agent)?;
    if messages[index + 1..]
        .iter()
        .any(|m| m.role == ChatRole::User)
    {
        return None;
    }
    let candidate = &messages[index];
    let tail = parse_reply_tail(&crate::ui::message_text::readable(candidate));
    tail.kind.asks_input().then_some(index)
}

/// Digit glyph size / line height of one digest row.
const ROW_SIZE: f32 = 13.0;
const ROW_LINE_HEIGHT: f32 = 20.0;

/// Paints the at-a-glance digest block directly under a reply body: a 1 px
/// [`theme::BORDER`] hairline, a small breather, and a rounded
/// [`theme::DIGEST_BG`] backdrop holding at most [`MAX_DIGEST_BULLETS`
/// bullet rows — the classified bullets when present, otherwise the single
/// lifted ask (covering `NextStep` / `FinalQuestion`). Rows are UNLABELED:
/// the first row is simply printed stronger so the ask leads without any
/// label word. Full row text wraps at the constrained width with no
/// ellipsis; the detector's five-row bound caps the height.
///
/// Kinds that do not ask for input (`NoReply`, `Plain`) return without
/// painting anything, so the caller gets a stable zero-chrome guarantee.
pub fn paint_open_ask(ui: &mut egui::Ui, tail: &ReplyTail) {
    if !tail.kind.asks_input() {
        return;
    }
    // (1) Hairline rule spanning the message column.
    let origin = ui.cursor().min;
    ui.painter().line_segment(
        [origin, origin + egui::vec2(ui.available_width(), 0.0)],
        egui::Stroke::new(1.0, theme::BORDER),
    );
    // (2) Breathing room between the rule and the backdrop.
    ui.add_space(4.0);
    // (3) The backdrop frame owning every row.
    let rows: Box<[&str]> = if !tail.bullets.is_empty() {
        tail.bullets.iter().map(String::as_str).collect()
    } else {
        match tail.ask.as_deref() {
            Some(row) => vec![row].into_boxed_slice(),
            // Defensive only: every input-seeking kind carries an ask.
            None => return,
        }
    };
    egui::Frame::NONE
        .fill(theme::DIGEST_BG)
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(10, 7))
        .show(ui, |ui| {
            for (leading, row) in rows.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.add(egui::Label::new(
                        egui::RichText::new('\u{2022}')
                            .size(ROW_SIZE)
                            .color(theme::TEXT_DIM),
                    ));
                    ui.add_space(6.0);
                    let mut text = egui::RichText::new(*row)
                        .size(ROW_SIZE)
                        .line_height(Some(ROW_LINE_HEIGHT))
                        .color(theme::TEXT);
                    if leading == 0 {
                        // Unlabeled lead: the first row IS the ask and its
                        // stronger format says so. `.strong()` plus the
                        // 0.01×size tracking that the in-tree Markdown fold
                        // already uses to express bold in a galley.
                        text = text.strong().extra_letter_spacing(0.01 * ROW_SIZE);
                    }
                    ui.add(egui::Label::new(text).wrap());
                });
            }
        });
}

/// The Digest classification, or `None` when the tail does not fit the
/// digest grammar; the caller then continues with the fallback paths.
fn digest_tail(prose: &str) -> Option<ReplyTail> {
    // Byte spans of each line, terminators inclusive, so bodies can be
    // sliced straight out of the input.
    let mut spans: Vec<(usize, usize)> = Vec::new();
    let mut cursor = 0usize;
    for piece in prose.split_inclusive('\n') {
        let len = piece.len();
        if len > 0 {
            spans.push((cursor, cursor + len));
        }
        cursor += len;
    }
    // The anchor is the LAST line whose trimmed text is exactly "---"
    // (ASCII; em-dash runs and `***` / `___` ornaments are not anchors).
    // Earlier anchors are never retried.
    let anchor = spans
        .iter()
        .rposition(|&(start, end)| prose[start..end].trim() == "---")?;
    let mut bullets = Vec::new();
    for &(start, end) in spans.iter().skip(anchor + 1) {
        let line = prose[start..end].trim();
        if line.is_empty() {
            continue;
        }
        // Any non-bullet line after the final rule disqualifies the whole
        // digest so the tail degrades instead of half-lifting.
        bullets.push(parse_bullet(line)?);
    }
    if !(1..=MAX_DIGEST_BULLETS).contains(&bullets.len()) {
        return None;
    }
    let (anchor_start, _) = spans[anchor];
    Some(ReplyTail {
        kind: TailKind::Digest,
        // Byte span from the start of the prose to the start of the anchor
        // line: interior bytes (CRLF, indentation) survive exactly as in
        // the legacy substring-derived summaries — no line re-joining.
        body: prose[..anchor_start].trim().to_string(),
        // A one-bullet digest maps 1:1 onto the card 'Your answer needed'
        // line: the first bullet IS the ask.
        ask: Some(bullets[0].clone()),
        no_reply: false,
        bullets,
    })
}

/// A digest bullet: a `- ` / `* ` / `+ ` opener (marker then at least one
/// ASCII space or tab and at least one non-whitespace character) or a
/// numbered `N. ` / `N) ` opener. Returns the text with the marker and
/// surrounding whitespace stripped (interior whitespace preserved), or
/// `None` when the line is not a bullet or exceeds `MAX_BULLET_CHARS`.
/// `line` must be a non-empty, trimmed line.
fn parse_bullet(line: &str) -> Option<String> {
    let bytes = line.as_bytes();
    let after_marker = if matches!(bytes[0], b'-' | b'*' | b'+') {
        let rest = &line[1..];
        if !matches!(rest.bytes().next(), Some(b' ') | Some(b'\t')) {
            return None; // whitespace is obligatory after the marker
        }
        rest
    } else if bytes[0].is_ascii_digit() {
        let digits = line.find(|c: char| !c.is_ascii_digit()).unwrap_or(line.len());
        let (close, gap) = (
            bytes.get(digits).copied(),
            bytes.get(digits + 1).copied(),
        );
        if !matches!(close, Some(b'.') | Some(b')')) || !matches!(gap, Some(b' ') | Some(b'\t')) {
            return None;
        }
        &line[digits + 1..]
    } else {
        return None;
    };
    let text = after_marker.trim_start();
    if text.is_empty() {
        return None;
    }
    let text = text.trim_end();
    if text.chars().count() > MAX_BULLET_CHARS {
        return None;
    }
    Some(text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(body: &str, ask: Option<&str>) -> ReplyTail {
        ReplyTail {
            kind: TailKind::NextStep,
            body: body.into(),
            ask: ask.map(str::to_string),
            ..Default::default()
        }
    }
    fn nr(body: &str) -> ReplyTail {
        ReplyTail {
            kind: TailKind::NoReply,
            body: body.into(),
            no_reply: true,
            ..Default::default()
        }
    }
    fn fq(body: &str, ask: &str) -> ReplyTail {
        ReplyTail {
            kind: TailKind::FinalQuestion,
            body: body.into(),
            ask: Some(ask.to_string()),
            ..Default::default()
        }
    }
    fn pl(body: &str) -> ReplyTail {
        ReplyTail {
            kind: TailKind::Plain,
            body: body.into(),
            ..Default::default()
        }
    }
    fn dg(body: &str, bullets: &[&str]) -> ReplyTail {
        ReplyTail {
            kind: TailKind::Digest,
            body: body.into(),
            ask: Some(bullets[0].to_string()),
            bullets: bullets.iter().map(|b| b.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn corpus_locks_all_five_kinds_and_the_legacy_quirks() {
        let corpus: Vec<(&str, ReplyTail)> = vec![
            // Legacy NextStep marker: tolerant of bold dressing, case
            // sensitive, first occurrence only, empty post-marker → no ask.
            (
                "SSO is recorded.\nYour next step: Should guests use SSO too?",
                ts("SSO is recorded.", Some("Should guests use SSO too?")),
            ),
            (
                "SSO is recorded.\n**Your next step:** Should guests use SSO too?",
                ts("SSO is recorded.", Some("Should guests use SSO too?")),
            ),
            ("Done here.\nYour next step:", ts("Done here.", None)),
            (
                "Start.\nYour next step: first ask\nYour next step: second ask",
                ts("Start.", Some("first ask\nYour next step: second ask")),
            ),
            ("Only the marker survives.", pl("Only the marker survives.")),
            (
                "Your next step:",
                ts("", None),
            ),
            // Casing and mid-phrase occurrences never trigger the markers.
            (
                "a?\nyour next step: pick a vendor",
                pl("a?\nyour next step: pick a vendor"),
            ),
            (
                "Still working. No reply needed. Continuing now.",
                pl("Still working. No reply needed. Continuing now."),
            ),
            // Terminal closeouts; trailing newlines are ignored by trim_end.
            (
                "SSO and MFA are confirmed.\nNo reply needed.",
                nr("SSO and MFA are confirmed."),
            ),
            (
                "SSO and MFA are confirmed.\nNo reply needed.\n\n",
                nr("SSO and MFA are confirmed."),
            ),
            // The marker split outranks the closeout.
            (
                "Ledger updated. Your next step: confirm the fee. No reply needed.",
                ts("Ledger updated.", Some("confirm the fee. No reply needed.")),
            ),
            // The marker also outranks the digest grammar for future dual-shape replies.
            (
                "done.\nYour next step: pick vendor\n\n---\n- Vendor A",
                ts("done.", Some("pick vendor\n\n---\n- Vendor A")),
            ),
            // Final-`?` fallback: only the FINAL non-empty line can win.
            ("Anything else?", fq("", "Anything else?")),
            ("Patch landed.\nDeploy tonight?", fq("Patch landed.\nDeploy tonight?", "Deploy tonight?")),
            (
                "Asked earlier?\nNot done yet",
                pl("Asked earlier?\nNot done yet"),
            ),
            ("a?\nb!", pl("a?\nb!")),
            // Digest: last anchor, bullets verbatim in order, ask = first bullet.
            (
                "Draft ready.\n\n---\n- Enable SSO for all guests?\n- Recommended: yes, effective Monday.\n- Impact notes: CLR-021",
                dg(
                    "Draft ready.",
                    &[
                        "Enable SSO for all guests?",
                        "Recommended: yes, effective Monday.",
                        "Impact notes: CLR-021",
                    ]
                ),
            ),
            (
                "Prepared.\n---\n- Ready to ship?",
                dg("Prepared.", &["Ready to ship?"]),
            ),
            (
                "Menu.\n---\n* Pick a route\n+ Keep the cache\n1. Freeze schema v2\n2) Log the rollback",
                dg(
                    "Menu.",
                    &["Pick a route", "Keep the cache", "Freeze schema v2", "Log the rollback"]
                ),
            ),
            // An earlier rule is swallowed into the body bytes.
            (
                "Intro line.\n---\nBridge note.\n---\n- Alpha\n- Beta",
                dg("Intro line.\n---\nBridge note.", &["Alpha", "Beta"]),
            ),
            // Interior CRLF survives untouched (byte span, no line re-joining).
            (
                "Line one.\r\nLine two.\n\n---\n- Ship it?\n- Ref: CLR-021",
                dg("Line one.\r\nLine two.", &["Ship it?", "Ref: CLR-021"]),
            ),
            // Malformed tails degrade wholesale; no fragment is lifted.
            (
                "Summary copy.\n---\nSee appendix nine.\n- Alpha",
                pl("Summary copy.\n---\nSee appendix nine.\n- Alpha"),
            ),
            (
                "Setup.\n---\n-x tight marker\n- Ok",
                pl("Setup.\n---\n-x tight marker\n- Ok"),
            ),
            (
                "Draft ready.\n\n---\n",
                pl("Draft ready.\n\n---"),
            ), // zero bullets
            (
                "Draft ready.\n\n---\n- ",
                pl("Draft ready.\n\n---\n-"),
            ), // hung marker, no content
            // In-progress streaming prefixes stay deterministic and plain.
            ("Draft ready.\n\n-", pl("Draft ready.\n\n-")),
            // Decorative rules are not anchors: em-dash run, `***`, `___`.
            (
                "Note stands.\n\u{2014}\u{2014}\u{2014}\n- Alpha",
                pl("Note stands.\n\u{2014}\u{2014}\u{2014}\n- Alpha"),
            ),
            (
                "Note stands.\n***\n- Alpha",
                pl("Note stands.\n***\n- Alpha"),
            ),
            (
                "Note stands.\n___\n- Alpha",
                pl("Note stands.\n___\n- Alpha"),
            ),
            // Degenerate input: no panic, no unwrap on missing lines.
            ("", pl("")),
            ("   \n\t", pl("")),
        ];
        for (input, expected) in corpus {
            assert_eq!(
                parse_reply_tail(input),
                expected,
                "misclassified {input:?} (fields compared wholesale)"
            );
        }
    }

    #[test]
    fn cap_boundaries_flip_exactly_at_the_limits() {
        // A bullet of exactly 240 chars still digests; 241 degrades the block.
        let b240 = "x".repeat(MAX_BULLET_CHARS);
        let b241 = "x".repeat(MAX_BULLET_CHARS + 1);
        let in240 = format!("Seed.\n---\n- {b240}");
        let in241 = format!("Seed.\n---\n- {b241}");
        assert_eq!(parse_reply_tail(&in240), dg("Seed.", &[b240.as_str()]));
        assert_eq!(parse_reply_tail(&in241), pl(in241.as_str()));

        // The final-`?` cap counts the UN-trimmed line: 280 chars with
        // leading spaces still lifts; 281 degrades.
        let q280 = format!("{} ?", "q".repeat(MAX_QUESTION_CHARS - 2));
        assert_eq!(q280.chars().count(), MAX_QUESTION_CHARS);
        assert_eq!(parse_reply_tail(&q280), fq("", q280.as_str()));
        let padded280 = format!("{}{} ?", " ".repeat(10), "q".repeat(MAX_QUESTION_CHARS - 12));
        assert_eq!(padded280.chars().count(), MAX_QUESTION_CHARS);
        assert_eq!(parse_reply_tail(&padded280), fq("", padded280.trim()));
        let q281 = format!("{} ?", "q".repeat(MAX_QUESTION_CHARS - 1));
        assert_eq!(q281.chars().count(), MAX_QUESTION_CHARS + 1);
        assert_eq!(parse_reply_tail(&q281), pl(q281.as_str()));

        // Exactly five bullets digest; a sixth bullet degrades the block.
        let five: Vec<String> = (1..=MAX_DIGEST_BULLETS)
            .map(|n| format!("Item {n}"))
            .collect();
        let six: Vec<String> = (1..=MAX_DIGEST_BULLETS + 1)
            .map(|n| format!("Item {n}"))
            .collect();
        let five_block = format!("Deck.\n---\n{}", five.iter().map(|b| format!("- {b}")).collect::<Vec<_>>().join("\n"));
        let five_view: Vec<&str> = five.iter().map(String::as_str).collect();
        assert_eq!(parse_reply_tail(&five_block), dg("Deck.", &five_view));
        let six_block = format!("Deck.\n---\n{}", six.iter().map(|b| format!("- {b}")).collect::<Vec<_>>().join("\n"));
        assert_eq!(parse_reply_tail(&six_block), pl(six_block.as_str()));
    }

    /// `asks_input` partitions the five kinds exactly: the three input-seeking
    /// shapes ask, the two settled shapes do not.
    #[test]
    fn asks_input_partitions_the_kinds() {
        assert!(TailKind::Digest.asks_input());
        assert!(TailKind::NextStep.asks_input());
        assert!(TailKind::FinalQuestion.asks_input());
        assert!(!TailKind::NoReply.asks_input());
        assert!(!TailKind::Plain.asks_input());
    }

    /// Selector contract, vector-locked: last agent wins, a USER message after
    /// it clears the lift, a SYSTEM notice after it preserves it, and the
    /// candidate tail itself must still seek input.
    #[test]
    fn open_ask_selector_locks_last_agent_and_user_clears_system_preserves() {
        const DIGEST: &str = "Draft ready.\n\n---\n- Which vendor shall we bind?\n- Recommended: Aurora, effective Monday.\n- Pointer: CLR-021 impact notes.";
        const PLAIN: &str = "Quiet workday, nothing blocked.";
        const NEXT_STEP: &str = "SSO is recorded.\nYour next step: Should guests use SSO too?";
        const FINAL_Q: &str = "Patch landed.\nDeploy tonight?";
        const NO_REPLY: &str = "SSO and MFA are confirmed.\nNo reply needed.";
        let u = |text: &str| ChatMessage::new(ChatRole::User, text, None);
        let a = |text: &str| ChatMessage::new(ChatRole::Agent, text, None);
        let s = |text: &str| ChatMessage::new(ChatRole::System, text, None);

        let vectors: Vec<(Vec<ChatMessage>, Option<usize>)> = vec![
            (vec![], None),                                // empty log
            (vec![u("Bind the vendor please.")], None),    // user only
            (vec![a(PLAIN)], None),                        // agent plain
            (vec![a(DIGEST)], Some(0)),                    // agent valid digest
            (vec![a(DIGEST), u("Go with Aurora.")], None), // digest then user: answered
            (
                vec![a(DIGEST), s("Update: ledger synced.")], // digest then System notice: still open
                Some(0),
            ),
            (
                // older open digest, final agent plain
                vec![u("one"), a(NEXT_STEP), u("two"), a(PLAIN)],
                None,
            ),
            (
                // digest then digest: LAST agent wins
                vec![a(NEXT_STEP), a(DIGEST)],
                Some(1),
            ),
            (vec![a(FINAL_Q)], Some(0)), // FinalQuestion-final log
            (vec![a(NO_REPLY)], None),   // NoReply-final log
            // Settled tail buried, reopened by a fresher seeking agent.
            (vec![a(NO_REPLY), a(FINAL_Q)], Some(1)),
            // A user answer after ANY settling noise still clears.
            (vec![a(DIGEST), s("Update."), u("Go.")], None),
        ];
        for (messages, expected) in vectors {
            assert_eq!(
                open_ask_index(&messages),
                expected,
                "selector drifted for {messages:?}"
            );
        }
    }

    /// Exclusion contract: kinds that do not seek input paint ZERO tail
    /// chrome — no backdrop fill, no BORDER hairline of any thickness.
    #[test]
    fn paint_open_ask_paints_zero_chrome_when_the_tail_does_not_seek_input() {
        for prose in [
            "All green, nothing blocked.",                  // Plain
            "SSO and MFA are confirmed.\nNo reply needed.", // NoReply
        ] {
            let tail = parse_reply_tail(prose);
            let ctx = egui::Context::default();
            ctx.set_visuals(theme::packet_visuals());
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                egui::CentralPanel::default().show(ui, |ui| paint_open_ask(ui, &tail));
            });
            let backdrops = output
                .shapes
                .iter()
                .filter(|clipped| matches!(&clipped.shape, egui::Shape::Rect(r) if r.fill == theme::DIGEST_BG))
                .count();
            let hairlines = output
                .shapes
                .iter()
                .filter(|clipped| matches!(&clipped.shape, egui::Shape::LineSegment { stroke, .. } if stroke.color == theme::BORDER))
                .count();
            assert_eq!(backdrops, 0, "no backdrop for {prose:?}");
            assert_eq!(hairlines, 0, "no hairline for {prose:?}");
            output.textures_delta.clear();
        }
    }
}
