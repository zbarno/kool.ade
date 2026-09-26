use super::*;

/// Task-only activity survives reconnects without becoming a tracked artifact.
pub fn load_activity(repo: &Path, ticket: &str) -> Option<crate::harness::LiveProgress> {
    serde_json::from_slice(&fs::read(state_dir(repo, ticket).ok()?.join("activity.json")).ok()?)
        .ok()
}
pub fn save_activity(
    repo: &Path,
    ticket: &str,
    activity: &crate::harness::LiveProgress,
) -> anyhow::Result<()> {
    let dir = state_dir(repo, ticket)?;
    fs::create_dir_all(&dir)?;
    // Persist a bounded view: in-memory snapshots keep the full history, but
    // the on-disk snapshot keeps only trailing windows so a long turn cannot
    // bloat activity.json (quadratic rewrites of ever-larger files were a
    // major source of disk wear).
    crate::artifacts::atomic_write_bytes(
        &dir.join("activity.json"),
        &serde_json::to_vec(&trim_for_persist(activity))?,
    )
}

/// Bounds for the persisted activity snapshot (see [`save_activity`]).
pub(super) const ACTIVITY_MAX_POSTS: usize = 200;
pub(super) const ACTIVITY_MAX_POST_CHARS: usize = 2_000;
pub(super) const ACTIVITY_MAX_FIELD_CHARS: usize = 32_000;

/// Build the bounded, persistence-shaped copy of a live snapshot.
/// Pure: the caller's in-memory state is never mutated.
pub(super) fn trim_for_persist(
    progress: &crate::harness::LiveProgress,
) -> crate::harness::LiveProgress {
    let mut out = progress.clone();
    if out.posts.len() > ACTIVITY_MAX_POSTS {
        let drop = out.posts.len() - ACTIVITY_MAX_POSTS;
        out.posts.drain(..drop);
    }
    for post in &mut out.posts {
        post.text = retain_suffix(&post.text, ACTIVITY_MAX_POST_CHARS);
    }
    out.thoughts = retain_suffix(&out.thoughts, ACTIVITY_MAX_FIELD_CHARS);
    out.response = retain_suffix(&out.response, ACTIVITY_MAX_FIELD_CHARS);
    out.specification = out
        .specification
        .as_deref()
        .map(|s| retain_suffix(s, ACTIVITY_MAX_FIELD_CHARS));
    out.activity = out
        .activity
        .as_deref()
        .map(|s| retain_suffix(s, ACTIVITY_MAX_FIELD_CHARS));
    out
}

/// Keep the LAST `max_chars` characters of `s` (the tail carries the newest
/// content), prefixing an elision marker when anything was dropped.
/// Character-boundary safe.
pub(super) fn retain_suffix(s: &str, max_chars: usize) -> String {
    let total = s.chars().count();
    if total <= max_chars {
        return s.to_owned();
    }
    let drop = total - max_chars;
    let cut = s.char_indices().nth(drop).map(|(idx, _)| idx).unwrap_or(0);
    format!("\u{2026}{}", &s[cut..])
}
