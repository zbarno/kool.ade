use crate::harness::safe_activity_text;

/// Bounds for the persisted activity snapshot (see [`save_activity`]).
pub(crate) const ACTIVITY_MAX_POSTS: usize = 200;
pub(crate) const ACTIVITY_MAX_POST_CHARS: usize = 2_000;
pub(crate) const ACTIVITY_MAX_FIELD_CHARS: usize = 32_000;

/// Build the bounded, persistence-shaped copy of a live snapshot.
/// Pure: the caller's in-memory state is never mutated.
pub(crate) fn trim_for_persist(
    progress: &crate::harness::LiveProgress,
) -> crate::harness::LiveProgress {
    let mut out = progress.clone();
    out.posts
        .retain(|post| !post.kind.eq_ignore_ascii_case("thinking"));
    if out.posts.len() > ACTIVITY_MAX_POSTS {
        let drop = out.posts.len() - ACTIVITY_MAX_POSTS;
        out.posts.drain(..drop);
    }
    for post in &mut out.posts {
        post.text = retain_suffix(&safe_activity_text(&post.text), ACTIVITY_MAX_POST_CHARS);
    }
    out.thoughts.clear();
    out.response = retain_suffix(&safe_activity_text(&out.response), ACTIVITY_MAX_FIELD_CHARS);
    out.specification = out
        .specification
        .as_deref()
        .map(|s| retain_suffix(&safe_activity_text(s), ACTIVITY_MAX_FIELD_CHARS));
    out.activity = out
        .activity
        .as_deref()
        .map(|s| retain_suffix(&safe_activity_text(s), ACTIVITY_MAX_FIELD_CHARS));
    out.activity_events
        .retain(|event| !event.kind.eq_ignore_ascii_case("thinking"));
    for event in &mut out.activity_events {
        event.content = retain_suffix(&safe_activity_text(&event.content), ACTIVITY_MAX_POST_CHARS);
    }
    if out.activity_events.len() > ACTIVITY_MAX_EVENTS {
        let remove = out.activity_events.len() - ACTIVITY_MAX_EVENTS;
        out.activity_events_truncated_before_ms =
            Some(out.activity_events[remove - 1].recorded_at_ms);
        out.activity_events.drain(..remove);
    }
    out
}

pub(super) const ACTIVITY_MAX_EVENTS: usize = 500;

/// Keep the LAST `max_chars` characters of `s` (the tail carries the newest
/// content), prefixing an elision marker when anything was dropped.
/// Character-boundary safe.
pub(crate) fn retain_suffix(s: &str, max_chars: usize) -> String {
    let total = s.chars().count();
    if total <= max_chars {
        return s.to_owned();
    }
    let drop = total - max_chars;
    let cut = s.char_indices().nth(drop).map(|(idx, _)| idx).unwrap_or(0);
    format!("\u{2026}{}", &s[cut..])
}
