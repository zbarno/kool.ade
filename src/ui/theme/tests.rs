use super::*;
use crate::domain::ItemKind::*;
use egui::Color32;

/// All five board discriminants return the CHG-002 recorded defaults
/// byte-for-byte (literal pins on purpose, so a future rebasing of the
/// ACCENT/PURPLE/etc palette constants fails loudly), and each item-kind
/// badge shares its foreground with the board hue of the same class.
#[test]
fn board_class_hues_match_recorded_defaults() {
    let kinds = [
        None,
        Some(Question),
        Some(Ambiguity),
        Some(Assumption),
        Some(Ownership),
    ];
    let recorded_defaults = [
        Color32::from_rgb(201, 209, 217),
        Color32::from_rgb(182, 108, 255),
        Color32::from_rgb(255, 194, 71),
        Color32::from_rgb(34, 184, 255),
        Color32::from_rgb(240, 106, 157),
    ];
    assert_eq!(kinds.len(), recorded_defaults.len());
    for (kind, expected) in kinds.into_iter().zip(recorded_defaults) {
        assert_eq!(board_hue(kind), expected, "unexpected hue for {:?}", kind);
    }
    assert_eq!(Question.badge_colors().1, board_hue(Some(Question)));
    assert_eq!(Ambiguity.badge_colors().1, board_hue(Some(Ambiguity)));
    assert_eq!(Assumption.badge_colors().1, board_hue(Some(Assumption)));
    assert_eq!(Ownership.badge_colors().1, board_hue(Some(Ownership)));
}

/// Stipulation ii, machine-guarded: DANGER red is not a card hue. The
/// sentinel is pinned first so the guard cannot pass vacuously, and all
/// five current discriminants - the four ItemKind arms plus the Task
/// `None` arm - reject red; add any new ItemKind variant to this list
/// (whatever arm it reaches) to keep the guard exhaustive.
#[test]
fn no_card_hues_claim_danger() {
    assert_eq!(DANGER, Color32::from_rgb(255, 83, 100));
    for kind in [
        None,
        Some(Question),
        Some(Ambiguity),
        Some(Assumption),
        Some(Ownership),
    ] {
        assert_ne!(
            board_hue(kind),
            DANGER,
            "board_hue({:?}) must never claim DANGER red",
            kind
        );
    }
}

/// Carry-forward of the incumbent's strongest surviving clause: no two
/// card hues may collide, catching a future single-arm typo that
/// reintroduces a duplicate even when both colliding values are
/// individually reasonable.
#[test]
fn board_hues_remain_pairwise_distinct() {
    let kinds = [
        None,
        Some(Question),
        Some(Ambiguity),
        Some(Assumption),
        Some(Ownership),
    ];
    let hues = kinds.map(board_hue);
    for (index, kind) in kinds.into_iter().enumerate() {
        assert!(
            !hues[..index].contains(&hues[index]),
            "duplicate card hue for {:?} at index {}",
            kind,
            index
        );
    }
}

/// Stipulation i + REQ-F20-2: every running card separates from its idle
/// twin in fill (0.22 vs 0.12 hue blend) AND in stroke (width or colour).
/// Specialisation: the running Task card earns the 2.5 px ACCENT stroke
/// over its 1.0 px idle (its base hue already equals ACCENT, so width is
/// the distinguishing channel), and every item class keeps 1.0 px while
/// promoting stroke colour to ACCENT against its idle class-hue stroke.
#[test]
fn board_frames_separate_active_from_idle_per_class() {
    let kinds = [
        None,
        Some(Question),
        Some(Ambiguity),
        Some(Assumption),
        Some(Ownership),
    ];
    for kind in kinds {
        let idle = board_frame(kind, false);
        let running = board_frame(kind, true);
        assert_ne!(running.fill, idle.fill, "fill shift lost for {:?}", kind);
        assert!(
            running.stroke.width != idle.stroke.width || running.stroke.color != idle.stroke.color,
            "running stroke indistinguishable from idle for {:?}",
            kind
        );
    }
    let idle_task = board_frame(None, false);
    let running_task = board_frame(None, true);
    assert_eq!(running_task.stroke.width, 1.5);
    assert_eq!(idle_task.stroke.width, 1.0);
    assert_eq!(running_task.stroke.color, board_hue(None));
    assert_eq!(idle_task.stroke.color, board_hue(None));
    for kind in [
        Some(Question),
        Some(Ambiguity),
        Some(Assumption),
        Some(Ownership),
    ] {
        let idle = board_frame(kind, false);
        let running = board_frame(kind, true);
        assert_eq!(running.stroke.color, board_hue(kind));
        assert_eq!(running.stroke.width, 1.5);
        assert_eq!(idle.stroke.color, board_hue(kind));
    }
}

#[test]
fn badge_impls_compile() {
    let (a, b) = crate::domain::Priority::High.badge_colors();
    let (c, d) = crate::domain::ItemKind::Question.badge_colors();
    let _ = (a, b, c, d);
}

/// CHG-003 story 4: the digest backdrop and the reserved chip tints hold
/// their recorded literals, and the backdrop ranks between BG (lowest) and
/// the card's answer-needed frame ACCENT_SOFT (highest) on every channel
/// while differing from both — it echoes the answer-needed cue without
/// blurring into either neighbor.
#[test]
fn digest_backdrop_and_reserved_chip_tints_hold_recorded_positions() {
    assert_eq!(DIGEST_BG, Color32::from_rgb(20, 50, 72));
    assert_eq!(CHIP_FILL, Color32::from_rgb(26, 42, 51));
    assert_eq!(CHIP_BORDER, Color32::from_rgb(60, 83, 94));
    for (a, b) in [
        (BG.r(), DIGEST_BG.r()),
        (BG.g(), DIGEST_BG.g()),
        (BG.b(), DIGEST_BG.b()),
    ] {
        assert!(a <= b, "DIGEST_BG must sit at or above BG");
    }
    for (mid, hi) in [
        (DIGEST_BG.r(), ACCENT_SOFT.r()),
        (DIGEST_BG.g(), ACCENT_SOFT.g()),
        (DIGEST_BG.b(), ACCENT_SOFT.b()),
    ] {
        assert!(mid <= hi, "DIGEST_BG must sit at or below ACCENT_SOFT");
    }
    assert_ne!(DIGEST_BG, BG);
    assert_ne!(DIGEST_BG, ACCENT_SOFT);
}
