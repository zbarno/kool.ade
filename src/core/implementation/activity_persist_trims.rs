use super::activity::{
    ACTIVITY_MAX_FIELD_CHARS, ACTIVITY_MAX_POST_CHARS, ACTIVITY_MAX_POSTS, retain_suffix,
    trim_for_persist,
};
use crate::harness::LivePost;

fn post(id: u64, text: String) -> LivePost {
    LivePost {
        id: (id, 0),
        kind: "assistant_message".into(),
        text,
    }
}

#[test]
fn suffix_keep_is_char_safe_and_marked() {
    // Multibyte content must never panic at a char boundary.
    let s = "あ".repeat(10);
    let kept = retain_suffix(&s, 4);
    assert_eq!(kept, format!("\u{2026}{}", "あ".repeat(4)));
    // Under the cap: unchanged, no marker.
    assert_eq!(retain_suffix("hello world", 100), "hello world");
    // Exact cap: unchanged.
    assert_eq!(retain_suffix("abcde", 5), "abcde");
    // Keeps the NEWEST tail, drops the head.
    assert_eq!(retain_suffix("0123456789", 4), "\u{2026}6789".to_string());
}

#[test]
fn trim_bounds_posts_and_fields_without_touching_input() {
    let big_post = "x".repeat(ACTIVITY_MAX_POST_CHARS * 3);
    let mut big = crate::harness::LiveProgress::default();
    for i in 0..(ACTIVITY_MAX_POSTS as u64 + 50) {
        big.posts.push(post(i, "line".repeat(100)));
    }
    big.posts.push(post(999_999, big_post.clone()));
    big.thoughts = "t".repeat(ACTIVITY_MAX_FIELD_CHARS * 2);
    big.response = "r".repeat(ACTIVITY_MAX_FIELD_CHARS * 2);
    big.specification = Some("s".repeat(ACTIVITY_MAX_FIELD_CHARS * 2));
    big.activity = Some("working".to_string());
    let before_posts = big.posts.len();
    let before_response = big.response.len();

    let trimmed = trim_for_persist(&big);

    assert_eq!(trimmed.posts.len(), ACTIVITY_MAX_POSTS);
    assert!(trimmed.posts.len() < before_posts);
    // Oldest 51 posts dropped (251 total -> 200 kept); newest kept.
    assert_eq!(trimmed.posts[0].id.0, 51);
    // Oversized post text bounded, tail preserved.
    let last = trimmed.posts.last().unwrap();
    assert!(last.text.chars().count() <= ACTIVITY_MAX_POST_CHARS + 1);
    assert!(
        last.text
            .ends_with(&"x".repeat(ACTIVITY_MAX_POST_CHARS.min(10)))
    );
    // Long fields bounded to tail-with-marker.
    for field in [
        &trimmed.thoughts,
        &trimmed.response,
        trimmed.specification.as_deref().unwrap(),
        trimmed.activity.as_deref().unwrap(),
    ] {
        assert!(field.chars().count() <= ACTIVITY_MAX_FIELD_CHARS + 1);
        assert!(field.starts_with('\u{2026}') || field.chars().count() <= ACTIVITY_MAX_FIELD_CHARS);
    }
    // Input snapshot untouched.
    assert_eq!(big.posts.len(), before_posts);
    assert_eq!(big.response.len(), before_response);
    // Round-trips through JSON the way save_activity writes it.
    let wire = serde_json::to_vec(&trimmed).unwrap();
    let back: crate::harness::LiveProgress = serde_json::from_slice(&wire).unwrap();
    assert_eq!(back, trimmed);
}
