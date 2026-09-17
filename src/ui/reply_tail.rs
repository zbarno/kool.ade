//! Shared reply-tail classifier. Operates on PRE-ENVELOPE reply prose only
//! (what `message_text::readable` hands out) and NEVER rewrites message
//! text: `ReplyTail::body` is a trimmed view of the supplied input and the
//! lifted ask/bullets are verbatim fragments of it. Painting is the
//! caller's concern; this module only decides the shape.
//!
//! Classification order is fixed and legacy-first:
//! `Your next step:` marker → `No reply needed.` closeout → the new
//! unlabeled digest → final-`?` fallback → plain. A malformed digest tail
//! degrades to the fallback/plain paths; a non-conforming tail is never
//! partially lifted. The classifier is pure and linear over the input, so
//! in-progress streaming prefixes classify deterministically.

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
}
