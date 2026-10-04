//! Shared reply-tail classifier. Operates on PRE-ENVELOPE reply prose only
//! (what `message_text::readable` hands out) and NEVER rewrites message
//! text: `ReplyTail::body` is a trimmed view of the supplied input and the
//! lifted ask/bullets are verbatim fragments of it. This module also hosts
//! the shared tail-block painter (`paint_open_ask`), the per-pane selector
//! (`open_ask_index`) deciding which stored agent message still owes the
//! operator an answer, and the CHG-003 story-5 tappable option-chip stack:
//! `digest_choices` (detects choice-bearing digest bullets),
//! `open_digest_choices` (feeds the card surface off the open ask),
//! `choice_label` (projects a short badge face from the full text),
//! `paint_chip_row` (draws the chips, returns the tapped index),
//! `join_choice` (types the full tapped text into a composer draft)
//! and `pin_caret_to_end` (parked-end caret + focus after a tap).
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
        let body = if ask == body {
            String::new()
        } else {
            body.to_string()
        };
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

// ==========================================================================
// Tappable option chips (CHG-003 story 5)
// ==========================================================================

/// One recognised choice carried by a digest bullet: its canonical display
/// TOKEN (the short badge face) plus the CLEAN, full bullet TEXT (what gets
/// typed into a composer and surfaced on hover). `text` is the exact cleaned
/// bullet text the detector produced (marker stripped, ends trimmed,
/// interiors preserved) — never re-derived from the token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OptionChoice {
    pub token: String,
    pub text: String,
}

/// Bound on the DESCRIPTOR portion of a badge label (tokens are exempt);
/// longer descriptors truncate with a single `…` marker. The composer and
/// the hover ALWAYS receive the full, untouched bullet text.
pub const DESCRIPTOR_CAP_CHARS: usize = 40;

/// Detect the choice-bearing bullets of a digest tail.
///
/// Case-insensitive, anchored rules with first-match-precedence (a bullet
/// carries AT MOST one token):
///  1. a leading ASCII-alphabetic word equal to `yes`/`no` \→ canonical `Yes`/`No`;
///  2. a leading word `option`, any spaces/tabs, then one or more digits \→ `Option N`;
///  3. a solitary ASCII letter immediately (no space) followed by `(`, `)` or `.` \→ the uppercased letter.
///
/// A bullet led by a decorative `Word:` labelling (≥2 alphabetic word, a
/// `:`, optional spaces) is peeled ONCE before anchoring, so
/// `Preferred: A(recommended)` resolves to its letter token `A` while
/// `NOTE: verify` / `Yesterday's plan` still match nothing.
///
/// Gate: chips surface only when 2..=6 bullets match; anything else makes
/// the WHOLE tail decline (a lone line is a suggestion, not a menu), and
/// non-`Digest` tails always yield zero choices. Matched bullets are
/// returned byte-identical in `OptionChoice::text`.
pub fn digest_choices(tail: &ReplyTail) -> Vec<OptionChoice> {
    if tail.kind != TailKind::Digest {
        return Vec::new();
    }
    let mut matches: Vec<OptionChoice> = Vec::new();
    for bullet in &tail.bullets {
        if let Some(token) = match_bullet_token(bullet) {
            matches.push(OptionChoice {
                token,
                text: bullet.clone(),
            });
        }
    }
    if (2..=6).contains(&matches.len()) {
        matches
    } else {
        Vec::new()
    }
}

/// The choices belonging to the OPEN ask (story-4 selector plus the
/// readability shield applied), for surfaces that work off the message log
/// directly (card task-chats). Settled/answered logs return none.
pub fn open_digest_choices(messages: &[ChatMessage]) -> Vec<OptionChoice> {
    open_ask_index(messages)
        .map(|index| {
            digest_choices(&parse_reply_tail(&crate::ui::message_text::readable(
                &messages[index],
            )))
        })
        .unwrap_or_default()
}

/// The badge-face label: the canonical token, then (only when a remainder
/// survives the separator-plus-leading-paren-group sweep) a SPACE and the
/// descriptor capped at [`DESCRIPTOR_CAP_CHARS`] characters with a `…`
/// marker. The sweep is what lets `Yes, bind Aurora effective Monday.`
/// settle into the quieter badge face `Yes bind Aurora effective Monday.`
/// while hover and insertion keep the full text.
pub fn choice_label(choice: &OptionChoice) -> String {
    let remainder = descriptor_remainder(&choice.text, &choice.token);
    let descriptor = polish_descriptor(remainder);
    if descriptor.is_empty() {
        return choice.token.clone();
    }
    let capped = truncate_char_capped(descriptor, DESCRIPTOR_CAP_CHARS);
    format!("{} {}", choice.token, capped)
}

/// Join a tapped choice onto a composer draft EXACTLY (verbatim append; no
/// trimming, no whitespace folding, no de-duplication): the choice's FULL
/// text is pushed onto the draft, prefixed by the single separator only
/// when the draft is non-empty and its last character is not already
/// whitespace.
pub fn join_choice(draft: &mut String, choice: &OptionChoice, separator: char) {
    let needs_join = draft
        .chars()
        .next_back()
        .is_some_and(|ch| !ch.is_whitespace());
    if needs_join {
        draft.push(separator);
    }
    draft.push_str(&choice.text);
}

/// Paints the tappable chip row beneath a lifted ask: a horizontally
/// WRAPPED row of corner-radius-10 rounded rectangles filled with
/// [`theme::CHIP_FILL`], stroked 1.0 [`theme::CHIP_BORDER`] (promoted to
/// [`theme::ACCENT`] on hover, only while enabled), carrying a tight 12 pt
/// badge label and minimal vertical breathing room. Enabled cells register
/// a full-text tooltip via `resp.on_hover_text` and return their INDEX on
/// click; disabled cells keep a dim label, forgo the hover promotion, and
/// absorb clicks. Empty input returns `None` immediately, laying out
/// nothing.
pub fn paint_chip_row(ui: &mut egui::Ui, choices: &[OptionChoice], enabled: bool) -> Option<usize> {
    if choices.is_empty() {
        return None;
    }
    ui.horizontal_wrapped(|row| {
        row.spacing_mut().item_spacing.x = 6.0;
        for (index, choice) in choices.iter().enumerate() {
            let label = choice_label(choice);
            let ink = if enabled {
                theme::TEXT
            } else {
                theme::TEXT_DIM
            };
            let galley = row
                .painter()
                .layout_no_wrap(label, egui::FontId::proportional(12.0), ink);
            let size = galley.size() + egui::vec2(14.0, 8.0);
            let sense = if enabled {
                egui::Sense::click()
            } else {
                egui::Sense::hover()
            };
            let (rect, response) = row.allocate_exact_size(size, sense);
            let stroke = egui::Stroke::new(
                1.0,
                if enabled && response.hovered() {
                    theme::ACCENT
                } else {
                    theme::CHIP_BORDER
                },
            );
            row.painter().rect(
                rect,
                10.0,
                theme::CHIP_FILL,
                stroke,
                egui::StrokeKind::Middle,
            );
            row.painter()
                .galley(rect.left_top() + egui::vec2(7.0, 4.0), galley, ink);
            if enabled {
                let response = response.on_hover_text(&choice.text);
                if response.clicked() {
                    return Some(index);
                }
            }
        }
        None
    })
    .inner
}

/// Caret pin: re-stores a CLONE of the editor state with both caret anchors
/// parked at the END of `draft`, then re-requests focus — so a tapped
/// choice lands right behind what just typed (instead of the caret hopping
/// to the start of a freshly-focused box) and the box is ready to keep
/// typing.
pub fn pin_caret_to_end(
    editor: &egui::widgets::text_edit::TextEditOutput,
    ctx: &egui::Context,
    draft: &str,
) {
    let end = egui::text::CCursor::new(draft.chars().count());
    let mut state = editor.state.clone();
    state
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::one(end)));
    state.store(ctx, editor.response.id);
    editor.response.request_focus();
}

// -------- private machinery ---------------------------------------------

/// Separators swept from the FRONT of a descriptor (surrounding whitespace
/// plus common punctuation). Parentheses are deliberately ABSENT here: a
/// leading paren is resolved by the balanced-group drop below (a blind
/// paren-stripping would strand the closing paren and make the group rule
/// dead code).
const DESCRIPTOR_SEPARATORS: [char; 13] = [
    ' ', '\t', '\n', '\r', ',', ';', ':', '.', '!', '?', '-', '\u{2013}', '\u{2014}',
];

/// Anchors a bullet's token; the first successful rule wins, and the
/// decorative-`Word:`-labelling peel gets exactly one retry.
fn match_bullet_token(bullet: &str) -> Option<String> {
    try_token_rules(bullet).or_else(|| peel_decorative_labelling(bullet).and_then(try_token_rules))
}

fn try_token_rules(candidate: &str) -> Option<String> {
    let word_len = candidate
        .bytes()
        .take_while(u8::is_ascii_alphabetic)
        .count();
    if word_len > 0 {
        let lowered = candidate[..word_len].to_ascii_lowercase();
        if lowered == "yes" {
            return Some("Yes".into());
        }
        if lowered == "no" {
            return Some("No".into());
        }
        if lowered == "option" {
            let after_word = &candidate[word_len..];
            let blanks = after_word
                .bytes()
                .take_while(|b| *b == b' ' || *b == b'\t')
                .count();
            let digits_span = &after_word[blanks..];
            let digit_count = digits_span.bytes().take_while(u8::is_ascii_digit).count();
            if digit_count > 0 {
                return Some(format!("Option {}", &digits_span[..digit_count]));
            }
        }
    }
    let mut chars = candidate.chars();
    if let (Some(letter), Some(after)) = (chars.next(), chars.next())
        && letter.is_ascii_alphabetic()
        && matches!(after, '(' | ')' | '.')
    {
        return Some(letter.to_ascii_uppercase().to_string());
    }
    None
}

/// Decorative `Word:` lead-ins (word of ≥2 letters, a direct `:`, at most
/// spaces after) are peeled exactly once so labelled menus
/// (`Preferred: A(recommended)`) still anchor on the real token. A one-
/// LETTER word is never peeled (it would shadow a letter token itself).
fn peel_decorative_labelling(bullet: &str) -> Option<&str> {
    let word_len = bullet.bytes().take_while(u8::is_ascii_alphabetic).count();
    if word_len >= 2 && word_len < bullet.len() && bullet.as_bytes()[word_len] == b':' {
        Some(bullet[word_len + 1..].trim_start())
    } else {
        None
    }
}

/// Bytes the canonical token consumed inside its source bullet. The
/// detector guarantees the alignment: `Option N` matched only against an
/// `<option><spaces/tabs><SAME digits>` prefix, and `Yes`/`No` against a
/// same-length case variant of their leading word.
fn token_prefix_len(text: &str, token: &str) -> usize {
    if token == "Yes" || token == "No" {
        token.len()
    } else if let Some(digits) = token.strip_prefix("Option ") {
        let after_word = &text[6.min(text.len())..];
        let blanks = after_word
            .bytes()
            .take_while(|b| *b == b' ' || *b == b'\t')
            .count();
        6 + blanks + digits.len()
    } else {
        1
    }
}

/// The remainder of the bullet after its token — the descriptor material.
fn descriptor_remainder<'a>(text: &'a str, token: &str) -> &'a str {
    let prefix = token_prefix_len(text, token).min(text.len());
    &text[prefix..]
}

/// Runs the two cleaning sweeps (separator strip, balanced leading-paren
/// group drop) until stable and returns the descriptor residue (possibly
/// empty → the label prints the bare token).
fn polish_descriptor(start: &str) -> &str {
    let mut rest = start;
    for _ in 0..4 {
        if let Some(inner) = drop_leading_paren_group(rest) {
            rest = inner;
            continue;
        }
        let swept = rest.trim_start_matches(|c: char| DESCRIPTOR_SEPARATORS.contains(&c));
        if swept.len() == rest.len() {
            break;
        }
        rest = swept;
    }
    rest
}

/// Drops the SMALLEST leading BALANCED `( … )` group when it closes within
/// 80 chars of the remainder; declines (leaving the text degraded-but-
/// intact) when no such close exists.
fn drop_leading_paren_group(rest: &str) -> Option<&str> {
    if !rest.starts_with('(') {
        return None;
    }
    let mut depth = 0i32;
    let mut chars_seen = 0usize;
    for (index, ch) in rest.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
        if depth == 0 {
            return Some(&rest[index + 1..]);
        }
        chars_seen += 1;
        if chars_seen >= 80 {
            return None;
        }
    }
    None
}

/// Caps to `cap` UNICODE characters (never splitting a scalar value),
/// appending a single `…` exactly when something was cut.
fn truncate_char_capped(text: &str, cap: usize) -> String {
    if text.chars().count() <= cap {
        return text.to_string();
    }
    let mut out: String = text.chars().take(cap).collect();
    out.push('\u{2026}');
    out
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
        let digits = line
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(line.len());
        let (close, gap) = (bytes.get(digits).copied(), bytes.get(digits + 1).copied());
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
            ("Your next step:", ts("", None)),
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
            (
                "Patch landed.\nDeploy tonight?",
                fq("Patch landed.\nDeploy tonight?", "Deploy tonight?"),
            ),
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
                    ],
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
                    &[
                        "Pick a route",
                        "Keep the cache",
                        "Freeze schema v2",
                        "Log the rollback",
                    ],
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
            ("Draft ready.\n\n---\n", pl("Draft ready.\n\n---")), // zero bullets
            ("Draft ready.\n\n---\n- ", pl("Draft ready.\n\n---\n-")), // hung marker, no content
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
        let padded280 = format!(
            "{}{} ?",
            " ".repeat(10),
            "q".repeat(MAX_QUESTION_CHARS - 12)
        );
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
        let five_block = format!(
            "Deck.\n---\n{}",
            five.iter()
                .map(|b| format!("- {b}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        let five_view: Vec<&str> = five.iter().map(String::as_str).collect();
        assert_eq!(parse_reply_tail(&five_block), dg("Deck.", &five_view));
        let six_block = format!(
            "Deck.\n---\n{}",
            six.iter()
                .map(|b| format!("- {b}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
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
            ctx.set_visuals(theme::koolade_visuals());
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

    // ------------------------------------------------------------------
    // CHG-003 story 5: tappable option chips.
    // ------------------------------------------------------------------

    fn opt(token: &str, text: &str) -> OptionChoice {
        OptionChoice {
            token: token.into(),
            text: text.into(),
        }
    }

    /// Detector table (test-plan vector 2): the case-insensitive
    /// yes/no · option-N · letter grammar, the decorative-`Word:`-labelling
    /// peel, the false-positive guard battery, and the 2..=6 all-or-nothing
    /// gate. Handcrafted tails exercise the full match band directly
    /// (the parser's bullet count caps at five, so six/seven match the
    /// detector through handcrafted input).
    #[test]
    fn digest_choices_apply_the_token_grammar_and_the_all_or_nothing_gate() {
        // Handcrafted: six match (band edge) and seven match (over-band)
        // plus kind-gated non-digest tails.
        let six = dg(
            "Six.",
            &[
                "A(one)", "B(two)", "C(three)", "D(four)", "Option 5", "No go",
            ],
        );
        assert_eq!(
            digest_choices(&six),
            vec![
                opt("A", "A(one)"),
                opt("B", "B(two)"),
                opt("C", "C(three)"),
                opt("D", "D(four)"),
                opt("Option 5", "Option 5"),
                opt("No", "No go"),
            ],
            "six matching bullets ride the band"
        );
        let seven = dg(
            "Seven.",
            &[
                "A(one)", "B(two)", "C(three)", "D(four)", "Option 5", "No go", "Z(nine)",
            ],
        );
        assert!(
            digest_choices(&seven).is_empty(),
            "seven matching bullets DECLINE wholesale"
        );
        // Non-Digest kinds ALWAYS decline, even a synthetically stuffed
        // bullet list of perfectly matching options — chips ride the fresh
        // marker'd digest and nowhere else.
        for kind in [
            TailKind::NextStep,
            TailKind::FinalQuestion,
            TailKind::NoReply,
            TailKind::Plain,
        ] {
            let tail = ReplyTail {
                kind,
                body: "Body.".into(),
                ask: Some("Should we?".into()),
                bullets: vec!["Yes, please.".into(), "No, thanks.".into()],
                ..Default::default()
            };
            assert!(
                digest_choices(&tail).is_empty(),
                "{kind:?} never feeds chips even with match-shaped bullets"
            );
        }

        // Parser-path vectors (prose → expected choices).
        let vectors: Vec<(&str, &str, Vec<OptionChoice>)> = vec![
            (
                "lower/UPPERCASE yes·no normalize to canonical tokens",
                "Pair.\n---\n- Yes, do it.\n- No, skip it.",
                vec![opt("Yes", "Yes, do it."), opt("No", "No, skip it.")],
            ),
            (
                "caps variants",
                "Pick.\n---\n- NO way.\n- YES sir.",
                vec![opt("No", "NO way."), opt("Yes", "YES sir.")],
            ),
            (
                "option N (any case, space-run absorbed into the token)",
                "Route.\n---\n- option 12 goes\n- Option  13 stays\n- OPTION 14 wins",
                vec![
                    opt("Option 12", "option 12 goes"),
                    opt("Option 13", "Option  13 stays"),
                    opt("Option 14", "OPTION 14 wins"),
                ],
            ),
            (
                "letter+adornment tokens (kept lowercase in text, uppercased in token)",
                "Choose.\n---\n- A(route)\n- B(plan)\n- C. third",
                vec![
                    opt("A", "A(route)"),
                    opt("B", "B(plan)"),
                    opt("C", "C. third"),
                ],
            ),
            (
                "bare lowercase letter bullet",
                "Low or high?\n---\n- b. low\n- A(high)",
                vec![opt("B", "b. low"), opt("A", "A(high)")],
            ),
            (
                "decorative Word: labelling peeled once (AC5 mixed-case row)",
                "Chose wisely.\n---\n- Which vendor shall we bind?\n- Preferred: A(recommended) — fast.\n- Option 2: slow but steady.\n- Details in CLR-021.",
                vec![
                    opt("A", "Preferred: A(recommended) — fast."),
                    opt("Option 2", "Option 2: slow but steady."),
                ],
            ),
            (
                "note-labelled bullet WITHOUT a token stays excluded",
                "Careful.\n---\n- NOTE: verify the port first\n- Yes, patch nightly\n- No, defer to Friday",
                vec![
                    opt("Yes", "Yes, patch nightly"),
                    opt("No", "No, defer to Friday"),
                ],
            ),
            (
                "Yesterday: and Never-/recommend-style prefixes never tokenize",
                "Redo?\n---\n- Yesterday's pickup slipped\n- Never mind.\n- Yes, redo it\n- No, let it ride",
                vec![opt("Yes", "Yes, redo it"), opt("No", "No, let it ride")],
            ),
            (
                "I recommend Postman: stays excluded (only the yes/no pair counts)",
                "Vendor.\n---\n- I recommend Postman.\n- Yes, bind it.\n- A nice compromise",
                vec![], // one match < 2 → whole tail declines
            ),
            (
                "option with no digits (option TBD) declines",
                "Standby.\n---\n- Option TBD\n- Option 2 solid",
                vec![], // one match < 2 → whole tail declines
            ),
            (
                "unclosed parenthesis in the adornment still yields the letter",
                "Bound?\n---\n- A(unclosed parenthesis\n- No, stop",
                vec![opt("A", "A(unclosed parenthesis"), opt("No", "No, stop")],
            ),
            (
                "terminal punctuation is NOT a letter delimiter (guard)",
                "Wrap.\n---\n- Which vendor shall we bind?",
                vec![], // anchored: a mid-word "d" before the final "." must NOT become a token
            ),
        ];
        for (name, prose, expected) in &vectors {
            let tail = parse_reply_tail(prose);
            assert_eq!(&digest_choices(&tail), expected, "vector: {name}");
        }
    }

    /// End-to-end through the parser (AC1 shape): a `ready` header, a
    /// `---`, then the option bullets; choices surface with byte-identical
    /// clean bullet text.
    #[test]
    fn digest_choices_surface_from_parsed_digest_prose_byte_identical() {
        let prose = "Ready.\n\n---\n- Which vendor shall we bind?\n- Yes, bind Aurora effective Monday.\n- No, keep Postman.";
        let tail = parse_reply_tail(prose);
        assert_eq!(tail.kind, TailKind::Digest);
        let choices = digest_choices(&tail);
        assert_eq!(
            choices,
            vec![
                opt("Yes", "Yes, bind Aurora effective Monday."),
                opt("No", "No, keep Postman."),
            ]
        );
    }

    /// The open-ask wiring: chips track the SAME selector the lifted ask
    /// uses, apply the readability shield to whatever message the selector
    /// picks, and return nothing when the log is settled or shield-broken.
    #[test]
    fn open_digest_choices_track_the_open_ask_selector_through_shield() {
        let u = |t: &str| ChatMessage::new(ChatRole::User, t, None);
        let a = |t: &str| ChatMessage::new(ChatRole::Agent, t, None);
        let s = |t: &str| ChatMessage::new(ChatRole::System, t, None);
        let digest = "Ready.\n\n---\n- Bind the vendor?\n- Yes, bind Aurora effective Monday.\n- No, keep Postman.";
        let expected = digest_choices(&parse_reply_tail(digest));
        assert_eq!(expected.len(), 2);

        let open = vec![u("Pick."), a(digest)];
        assert_eq!(
            open_digest_choices(&open),
            expected,
            "open digest serves its choices"
        );

        let answered = vec![u("Pick."), a(digest), u("Yes, Aurora.")];
        assert!(
            open_digest_choices(&answered).is_empty(),
            "settled log serves none"
        );

        let plain = vec![u("Status?"), a("All green, nothing blocked.")];
        assert!(
            open_digest_choices(&plain).is_empty(),
            "plain final serves none"
        );

        let sys_notice = vec![u("Pick."), a(digest), s("Maintenance window.")];
        assert_eq!(
            open_digest_choices(&sys_notice),
            expected,
            "system notices preserve openness"
        );

        // Shield-broken envelopes NEVER feed the chips (they classify Plain
        // and fail the selector anyway) — assert neither path leaks.
        let broken = vec![
            u("Pick."),
            a("{\"assistant_message\":\"Ready.\\n----\n- B?\\n- Yes\\n- No\"}"),
        ];
        assert!(open_digest_choices(&broken).is_empty());
    }

    /// Label projection: token + whitespace-delimited, separator-trimmed,
    /// leading-paren-group-dropped descriptor; the 40-char unicode-cap adds
    /// exactly one trailing `…` — and only when it fires. Bare tokens print
    /// alone.
    #[test]
    fn choice_label_projects_the_token_plus_cleaned_capped_descriptor() {
        assert_eq!(choice_label(&opt("Yes", "Yes")), "Yes");
        assert_eq!(
            choice_label(&opt("No", "No, keep Postman.")),
            "No keep Postman."
        );
        assert_eq!(choice_label(&opt("B", "b. low cost")), "B low cost");
        assert_eq!(
            choice_label(&opt("A", "A(recommended) — fast.")),
            "A fast.",
            "leading parenthesized group drops as a unit"
        );
        assert_eq!(
            choice_label(&opt("B", "B (backup) plan ready")),
            "B plan ready"
        );
        assert_eq!(
            choice_label(&opt("A", "A(preferred — fast)")),
            "A",
            "entirely parenthesised → bare token"
        );
        assert_eq!(
            choice_label(&opt("C", "C — the cautious route")),
            "C the cautious route"
        );
        assert_eq!(
            choice_label(&opt("A", "A(unclosed paren forever")),
            "A (unclosed paren forever",
            "unclosed paren degrades but stays intact"
        );

        let forty = "x".repeat(DESCRIPTOR_CAP_CHARS);
        let forty_one = "x".repeat(DESCRIPTOR_CAP_CHARS + 1);
        assert_eq!(
            choice_label(&opt("Option 1", &format!("Option 1: {forty}"))),
            format!("Option 1 {forty}"),
            "40-char descriptor is EXACTLY at the cap: no marker"
        );
        assert_eq!(
            choice_label(&opt("Option 1", &format!("Option 1: {forty_one}"))),
            format!("Option 1 {forty}\u{2026}"),
            "41 chars truncate at 40 plus ONE …"
        );
        // Multibyte descriptors count characters (not bytes) for the cap.
        let rich_base = "café ☕ 🚀 ".repeat(4); // 36 chars incl. trailing space
        let rich = rich_base.trim_end();
        let long_rich = format!("{rich} and much more trailing substance indeed");
        let label = choice_label(&opt("No", &format!("No, {long_rich}")));
        let desc = label.strip_prefix("No ").expect("token leads");
        assert!(
            desc.chars().count() > DESCRIPTOR_CAP_CHARS,
            "multibyte descriptor ran long and got capped: {desc:?}"
        );
        assert_eq!(desc.chars().count(), DESCRIPTOR_CAP_CHARS + 1);
        assert!(desc.ends_with('\u{2026}'));
    }

    /// Exact-append contract: verbatim join with a smart single separator,
    /// preserving multibyte and pre-existing whitespace byte-for-byte.
    #[test]
    fn join_choice_appends_full_text_with_smart_single_separator() {
        let choice = opt("Yes", "Yes, bind Aurora effective Monday.");

        let mut empty = String::new();
        join_choice(&mut empty, &choice, '\n');
        assert_eq!(empty, "Yes, bind Aurora effective Monday.");

        let mut nonws = "Short answer:".to_string();
        join_choice(&mut nonws, &choice, ' ');
        assert_eq!(nonws, "Short answer: Yes, bind Aurora effective Monday.");

        let mut trail_ws = "Short answer: ".to_string();
        join_choice(&mut trail_ws, &choice, ' ');
        assert_eq!(trail_ws, "Short answer: Yes, bind Aurora effective Monday.");

        let mut newline_sep = "Line one\nLine two".to_string();
        join_choice(&mut newline_sep, &choice, '\n');
        assert_eq!(
            newline_sep,
            "Line one\nLine two\nYes, bind Aurora effective Monday."
        );

        let mut nl_after_ws = "Line one\nLine two ".to_string();
        join_choice(&mut nl_after_ws, &choice, '\n');
        assert_eq!(
            nl_after_ws,
            "Line one\nLine two Yes, bind Aurora effective Monday."
        );

        let mut ws_only = "   ".to_string();
        join_choice(&mut ws_only, &choice, '\n');
        assert_eq!(ws_only, "   Yes, bind Aurora effective Monday.");

        let mut multibyte = "Grüße 🎉".to_string();
        join_choice(&mut multibyte, &choice, ' ');
        assert_eq!(multibyte, "Grüße 🎉 Yes, bind Aurora effective Monday.");

        // Composition law: result == prior + (separator|∅) + exact text.
        let prior = "Earlier thought.".to_string();
        let mut composed = prior.clone();
        join_choice(&mut composed, &choice, '\n');
        assert_eq!(composed, format!("{prior}\n{}", choice.text));
        assert!(composed.ends_with(&choice.text));
    }

    /// Headless visual-shape coverage (test-plan vectors 8/9 minus focus):
    /// radius-10 cells in CHIP_FILL, idle CHIP_BORDER strokes, hover
    /// promotion to ACCENT strictly for hovered ENABLED cells, click
    /// indices for enabled chips, absorption for disabled ones, and zero
    /// footprint for empty input.
    #[test]
    fn paint_chip_row_shapes_strokes_and_click_semantics() {
        let ctx = egui::Context::default();
        ctx.set_visuals(theme::koolade_visuals());
        let enabled = vec![
            opt("Yes", "Yes, bind Aurora effective Monday."),
            opt("No", "No, keep Postman."),
        ];
        let disabled_solo = vec![opt("Solo", "Sole survivor option.")];
        let yes_label = choice_label(&enabled[0]);
        let no_label = choice_label(&enabled[1]);
        let solo_label = choice_label(&disabled_solo[0]);

        let cells = |out: &egui::FullOutput| -> Vec<(egui::Rect, egui::Color32)> {
            out.shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Rect(rect)
                        if rect.fill == theme::CHIP_FILL
                            && rect.corner_radius == egui::CornerRadius::same(10_u8) =>
                    {
                        Some((rect.rect, rect.stroke.color))
                    }
                    _ => None,
                })
                .collect()
        };
        let texts = |out: &egui::FullOutput| -> Vec<String> {
            out.shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                    _ => None,
                })
                .collect()
        };
        let hit_enabled = std::cell::Cell::new(None);
        let hit_disabled = std::cell::Cell::new(None);
        let paint_rows = |ui: &mut egui::Ui| {
            hit_enabled.set(paint_chip_row(ui, &enabled, true));
            hit_disabled.set(paint_chip_row(ui, &disabled_solo, false));
            assert!(
                paint_chip_row(ui, &[], true).is_none(),
                "empty input paints nothing"
            );
        };
        let frame = |events: Vec<egui::Event>| -> egui::FullOutput {
            ctx.run_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ui| {
                    egui::CentralPanel::default().show(ui, paint_rows);
                },
            )
        };
        let press_release = |pos: egui::Pos2| -> (Vec<egui::Event>, Vec<egui::Event>) {
            (
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: Default::default(),
                    },
                ],
                vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                }],
            )
        };

        let mut out = frame(Vec::new());
        let rects = cells(&out);
        assert_eq!(
            rects.len(),
            3,
            "two enabled + one disabled cell, empty row adds none"
        );
        assert!(
            rects.iter().all(|(_, c)| *c == theme::CHIP_BORDER),
            "idle strokes stay CHIP_BORDER"
        );
        let seen = texts(&out);
        assert!(
            seen.contains(&yes_label),
            "chip one projects {yes_label:?} in {seen:?}"
        );
        assert!(
            seen.contains(&no_label),
            "chip two projects {no_label:?} in {seen:?}"
        );
        assert!(
            seen.contains(&solo_label),
            "disabled cell projects {solo_label:?}"
        );
        out.textures_delta.clear();

        // Hover over chip one promotes ITS stroke to ACCENT only.
        let chip_centers: Vec<egui::Pos2> = rects.iter().map(|(r, _)| r.center()).collect();
        let mut hover = frame(vec![egui::Event::PointerMoved(chip_centers[0])]);
        let hover_strokes = cells(&hover);
        assert_eq!(
            hover_strokes[0].1,
            theme::ACCENT,
            "hovered enabled chip earns the accent stroke"
        );
        assert_eq!(hover_strokes[1].1, theme::CHIP_BORDER);
        assert_eq!(
            hover_strokes[2].1,
            theme::CHIP_BORDER,
            "disabled cell ignores hover promotion"
        );
        hover.textures_delta.clear();

        // Clicking chip one (move, press, RELEASE far away… no: release over it).
        let (press_evts, release_evts) = press_release(chip_centers[0]);
        let _ = frame(press_evts);
        assert!(hit_enabled.get().is_none(), "press alone is not a click");
        let _ = frame(release_evts);
        assert_eq!(hit_enabled.get(), Some(0), "release over chip 0 → index 0");
        let _ = frame(Vec::new());
        assert_eq!(
            hit_disabled.get(),
            None,
            "no click ever reached the disabled row"
        );

        // Second enabled chip reports index 1.
        let (press2, release2) = press_release(chip_centers[1]);
        let _ = frame(press2);
        let _ = frame(release2);
        assert_eq!(hit_enabled.get(), Some(1), "release over chip 1 → index 1");

        // Disabled cell absorbs its whole gesture.
        let (press3, release3) = press_release(chip_centers[2]);
        let _ = frame(press3);
        let _ = frame(release3);
        assert_eq!(hit_disabled.get(), None, "disabled chips swallow clicks");
    }

    /// Caret pin unit proof: typing after a pinned editor continues BEHIND
    /// the pre-filled draft (an unpinned, freshly-focused box would put the
    /// keystroke at the front).
    #[test]
    fn pin_caret_parks_subsequent_typing_at_the_inserted_end() {
        let ctx = egui::Context::default();
        let mut draft = "abc".to_string();
        // Frame 1: create the editor and park the caret.
        let mut first = ctx.run_ui(egui::RawInput::default(), |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                let editor = egui::TextEdit::multiline(&mut draft)
                    .id_salt("chip_pin_probe")
                    .show(ui);
                pin_caret_to_end(&editor, ui.ctx(), &draft);
            });
        });
        first.textures_delta.clear();
        // Frame 2: plain repaint grants the focus request.
        let mut second = ctx.run_ui(egui::RawInput::default(), |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                egui::TextEdit::multiline(&mut draft)
                    .id_salt("chip_pin_probe")
                    .show(ui);
            });
        });
        second.textures_delta.clear();
        // Frame 3: a keystroke must land at the end.
        let mut third = ctx.run_ui(
            egui::RawInput {
                events: vec![egui::Event::Text("X".into())],
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    egui::TextEdit::multiline(&mut draft)
                        .id_salt("chip_pin_probe")
                        .show(ui);
                });
            },
        );
        third.textures_delta.clear();
        assert_eq!(
            draft, "abcX",
            "pinned caret types behind the draft, not ahead"
        );
    }
}
