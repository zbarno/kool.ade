//! User-specific question routing under the D-14 law (approved
//! specification §8, normative "Routing (D-14 law)").
//!
//! An open item is POSEABLE to the seated operator iff one of four rules
//! holds:
//!
//! 1. **Broadcast** — the item's category belongs to the structural General
//!    broadcast (General reaches everyone).
//! 2. **Direct address** — `assigned_to` names the user directly or one of
//!    the user's groups.
//! 3. **Owned lane** — the item's category is explicitly configured to that
//!    user: a sole personal owner, or a member of the owning group.
//! 4. **Seat inheritance** — the item's category has NO explicit owner at
//!    all; the seated git-identified operator inherits the lane.
//!
//! VETO (outranks direct address): a lane CLAIMED BY OTHER HOLDERS is never
//! posed to this user — even when the item's `assigned_to` names them.
//!
//! Claims are judged PER USER: a member string equal (trimmed, case-folded)
//! to the user's name marks sole ownership; a member string equal to one of
//! the user's groups marks shared ownership. The person-versus-group
//! distinction is semantic, not syntactic — no global registry is needed.
//!
//! The app ENFORCES this law on the agent's `next_question_id` at
//! validation: a choice that violates it rejects the ENTIRE TURN — nothing
//! is saved.

use crate::domain::{
    AssignmentMatch, CurrentUser, GUEST_NAME, OpenItem, Stakeholders, is_general_category,
};

/// Explanation of why an item may be posed to the current user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eligibility {
    ByName,
    ByGroup,
    General,
    /// Rule 3: the user is the sole person owning the item's category.
    SoleOwner,
    /// Rule 3: the user's group owns the item's category (shared).
    SharedOwner,
    /// Rule 4: the category has no explicit owner; the seated (non-guest)
    /// operator inherits it.
    SeatInherited,
    /// Not this user's item (visible in panel only; the veto or the address
    /// axis closed the door).
    NotEligible,
}

impl Eligibility {
    pub const fn is_eligible(self) -> bool {
        !matches!(self, Self::NotEligible)
    }
}

/// How a lane's configured holders relate to ONE user (private; the
/// verdict order in [`evaluate`] is fixed around it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LaneClaim {
    /// No entry for the category, or an entry with an EMPTY member list
    /// (the seeded-repo shape: a brand-new config lists categories with
    /// "(no owner configured)"). Seat-inheritable by the non-guest seat.
    Unowned,
    /// A member string equals the user's name — the user ALONE owns the
    /// category (the person match takes precedence over any group member).
    SoleMine,
    /// No name hit, but a member string matches one of the user's groups —
    /// the user CO-HOLDS the category with the group's other members.
    SharedMine,
    /// The lane is claimed by other holders: never poseable to this user.
    ClaimedByOthers,
}

/// Classify one user's relation to a category's configured holders.
fn lane_claim(category: &str, user: &CurrentUser, stakes: &Stakeholders) -> LaneClaim {
    let Some(entry) = stakes.find(category) else {
        return LaneClaim::Unowned;
    };
    if entry.members.is_empty() {
        return LaneClaim::Unowned;
    }
    let me = user.name.trim().to_ascii_lowercase();
    // Person match first: sole ownership wins over any shared claim, so
    // "Morgan" classifies SoleMine for Morgan even next to a group member.
    if entry
        .members
        .iter()
        .any(|m| m.trim().to_ascii_lowercase() == me)
    {
        return LaneClaim::SoleMine;
    }
    if entry.members.iter().any(|m| {
        let holder = m.trim().to_ascii_lowercase();
        user.groups
            .iter()
            .any(|g| g.trim().to_ascii_lowercase() == holder)
    }) {
        return LaneClaim::SharedMine;
    }
    LaneClaim::ClaimedByOthers
}

/// True when the seat cannot receive anything beyond broadcasts: the
/// neutral guest, or an unset identity (defensive — production seats are
/// always settled to one or the other).
fn gated_seat(user: &CurrentUser) -> bool {
    user.name.trim().eq_ignore_ascii_case(GUEST_NAME) || !user.is_set()
}

/// Evaluate one item against the seated user UNDER THE D-14 LAW.
///
/// Verdict order is FIXED: (1) General broadcast; (2) the other-holder veto
/// (BEFORE the address axis — a sole-owned lane is never posed to anyone
/// else, not even on direct address); (3) direct name or group address;
/// (4) nobody is addressed to us, so the lane's own claim decides: sole
/// owner, shared owner, or seat inheritance for unowned lanes (guests
/// inherit nothing — their experience is a strict subset of the law).
pub fn evaluate(item: &OpenItem, user: &CurrentUser, stakes: &Stakeholders) -> Eligibility {
    if is_general_category(&item.category) {
        return Eligibility::General;
    }
    let claim = lane_claim(&item.category, user, stakes);
    if claim == LaneClaim::ClaimedByOthers {
        return Eligibility::NotEligible;
    }
    match item
        .assigned_to
        .as_deref()
        .map(|a| user.matches_assignment(a))
    {
        Some(AssignmentMatch::ByName) => return Eligibility::ByName,
        Some(AssignmentMatch::ByGroup) => return Eligibility::ByGroup,
        _ => {}
    }
    match claim {
        LaneClaim::SoleMine => Eligibility::SoleOwner,
        LaneClaim::SharedMine => Eligibility::SharedOwner,
        LaneClaim::Unowned => {
            if gated_seat(user) {
                Eligibility::NotEligible
            } else {
                Eligibility::SeatInherited
            }
        }
        LaneClaim::ClaimedByOthers => unreachable!("vetoed above"),
    }
}

/// Filter a queue down to what the user may be interviewed with.
pub fn eligible_items<'a>(
    items: &'a [OpenItem],
    user: &CurrentUser,
    stakes: &Stakeholders,
) -> Vec<&'a OpenItem> {
    items
        .iter()
        .filter(|item| {
            item.status == crate::domain::ItemStatus::Open
                && evaluate(item, user, stakes).is_eligible()
                && !has_open_prerequisite(items, item)
        })
        .collect()
}

pub fn has_open_prerequisite(items: &[OpenItem], item: &OpenItem) -> bool {
    item.blocked_by.iter().any(|dependency| {
        items.iter().any(|candidate| {
            candidate.id == *dependency && candidate.status == crate::domain::ItemStatus::Open
        })
    })
}

/// The single question the agent SHOULD pose next: highest priority, then
/// earliest ID. Ownership-gap items never count as interview questions.
pub fn recommended_next<'a>(
    items: &'a [OpenItem],
    user: &CurrentUser,
    stakes: &Stakeholders,
) -> Option<&'a OpenItem> {
    eligible_items(items, user, stakes)
        .into_iter()
        .filter(|i| !i.is_ownership_gap())
        .min_by_key(|i| (i.priority.rank(), i.id.clone()))
}

/// Stable 0-3 line digest of the lanes this SEAT may lawfully serve, in
/// config order: sole-owned lanes, shared lanes (each marked with the via
/// group), and seat-inherited unowned lanes. Empty string for an empty
/// config and for the guest seat, which inherits nothing. Fed to the
/// per-turn prompt so the harness sees exactly which questions it may
/// legally propose to THIS seat.
///
/// Label note (interpretation frozen): a MIXED owner list (person +
/// group) reads as “Sole-owned” for the named holder because the person
/// match takes precedence there — gating is identical for both variants
/// (poseable to the seat); only the caption differs.
pub fn describe_lanes(user: &CurrentUser, stakes: &Stakeholders) -> String {
    if gated_seat(user) || stakes.entries.is_empty() {
        return String::new();
    }
    let mut sole: Vec<String> = Vec::new();
    let mut shared: Vec<(String, String)> = Vec::new(); // (category, via group)
    let mut unowned: Vec<String> = Vec::new();
    let mut seen: Vec<String> = Vec::new(); // fold duplicate spellings
    for entry in &stakes.entries {
        if is_general_category(&entry.name) {
            continue; // structural broadcast; never a "lane" of this seat
        }
        // Hand-edited configs may repeat a category under drifting spellings;
        // the digest folds them (evaluate/find already do first-match).
        let key = entry.name.trim().to_ascii_lowercase();
        if !seen.iter().any(|s| s == &key) {
            seen.push(key);
        } else {
            continue;
        }
        match lane_claim(&entry.name, user, stakes) {
            LaneClaim::SoleMine => sole.push(entry.name.clone()),
            LaneClaim::SharedMine => {
                // First matching member that is one of the user's groups.
                let via = entry
                    .members
                    .iter()
                    .find_map(|m| {
                        let holder = m.trim().to_ascii_lowercase();
                        user.groups
                            .iter()
                            .find(|g| g.trim().to_ascii_lowercase() == holder)
                            .map(|g| g.trim().to_string())
                    })
                    .unwrap_or_else(|| "your group".into());
                shared.push((entry.name.clone(), via));
            }
            LaneClaim::Unowned => unowned.push(entry.name.clone()),
            LaneClaim::ClaimedByOthers => {}
        }
    }
    let mut lines: Vec<String> = Vec::new();
    if !sole.is_empty() {
        lines.push(format!("Sole-owned lanes: {}", sole.join(", ")));
    }
    if !shared.is_empty() {
        lines.push(format!(
            "Shared lanes: {}",
            shared
                .iter()
                .map(|(c, g)| format!("{c} (via {g})"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !unowned.is_empty() {
        lines.push(format!(
            "Seat-inherited unowned lanes: {}",
            unowned.join(", ")
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{CategoryOwners, ItemKind, Priority};

    // ---- the D-14 regression-matrix fixtures -------------------------------
    // Security is SOLE-owned by Morgan; QA is SHARED through the group
    // "QA Guild"; InfoSec and UX exist as entries with EMPTY member lists
    // (the seeded-repo shape) and are UNOWNED.
    fn stakes() -> Stakeholders {
        Stakeholders::new(vec![
            CategoryOwners::new("Security", vec!["Morgan".into()]),
            CategoryOwners::new("QA", vec!["QA Guild".into()]),
            CategoryOwners::new("InfoSec", Vec::new()),
            CategoryOwners::new("UX", Vec::new()),
        ])
    }

    fn item(id: &str, prio: Priority, category: &str, assignee: Option<&str>) -> OpenItem {
        OpenItem::new(
            id.into(),
            prio,
            ItemKind::Question,
            category.into(),
            assignee.map(String::from),
            format!("q-{id}"),
            "r".into(),
        )
    }

    fn ownership_item(id: &str, prio: Priority, category: &str) -> OpenItem {
        let mut o = item(id, prio, category, None);
        o.kind = ItemKind::Ownership;
        o
    }

    fn chair() -> CurrentUser {
        CurrentUser::new("Zach", Vec::new())
    }

    fn qa_member() -> CurrentUser {
        CurrentUser::new("Robin", vec!["QA Guild".into()])
    }

    fn morgan() -> CurrentUser {
        CurrentUser::new("Morgan", Vec::new())
    }

    fn guest() -> CurrentUser {
        CurrentUser::new(GUEST_NAME, Vec::new())
    }

    #[test]
    fn all_independent_decisions_are_eligible_while_dependents_wait() {
        let mut prerequisite = item("CLR-010", Priority::High, "General", None);
        let mut dependent = item("CLR-011", Priority::High, "General", None);
        dependent.blocked_by = vec![prerequisite.id.clone()];
        let independent = item("CLR-012", Priority::High, "General", None);
        let mut queue = vec![prerequisite.clone(), dependent, independent];
        let eligible = eligible_items(&queue, &chair(), &Stakeholders::default());
        assert_eq!(
            eligible
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            ["CLR-010", "CLR-012"]
        );

        prerequisite.status = crate::domain::ItemStatus::Resolved;
        queue[0] = prerequisite;
        let eligible = eligible_items(&queue, &chair(), &Stakeholders::default());
        assert_eq!(
            eligible
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            ["CLR-011", "CLR-012"]
        );
    }

    // ---- rule 1: the General broadcast ------------------------------------

    #[test]
    fn general_reaches_every_seat_including_guest() {
        let q = item("CLR-001", Priority::Blocking, "General", None);
        assert_eq!(evaluate(&q, &chair(), &stakes()), Eligibility::General);
        assert_eq!(evaluate(&q, &morgan(), &stakes()), Eligibility::General);
        assert_eq!(
            evaluate(&q, &guest(), &Stakeholders::default()),
            Eligibility::General
        );
        assert_eq!(
            recommended_next(std::slice::from_ref(&q), &guest(), &Stakeholders::default())
                .map(|r| r.id.as_str()),
            Some("CLR-001")
        );
    }

    // ---- rule 2: direct name or group address -----------------------------

    #[test]
    fn direct_address_grants_by_name_and_by_group() {
        let by_name = item("CLR-002", Priority::High, "InfoSec", Some("zach "));
        assert_eq!(evaluate(&by_name, &chair(), &stakes()), Eligibility::ByName);

        let by_group = item("CLR-003", Priority::High, "QA", Some("QA GUILD"));
        assert_eq!(
            evaluate(&by_group, &qa_member(), &stakes()),
            Eligibility::ByGroup
        );
        // The address axis still binds for non-members of that group…
        assert_eq!(
            evaluate(&by_group, &chair(), &stakes()),
            Eligibility::NotEligible
        );

        // …and the guest seat KEEPS direct address (the legacy experience is
        // a strict subset of the law — guests lose only inheritance).
        let addressed_guest = item("CLR-003b", Priority::High, "InfoSec", Some(GUEST_NAME));
        assert_eq!(
            evaluate(&addressed_guest, &guest(), &stakes()),
            Eligibility::ByName
        );
    }

    // ---- rule 3: owned lanes ----------------------------------------------

    #[test]
    fn sole_owner_is_poseable_on_their_own_lane() {
        let q = item("CLR-004", Priority::High, "Security", None);
        assert_eq!(evaluate(&q, &morgan(), &stakes()), Eligibility::SoleOwner);
        assert_eq!(
            recommended_next(std::slice::from_ref(&q), &morgan(), &stakes()).map(|r| r.id.as_str()),
            Some("CLR-004")
        );
        // …and the SAME lane closes for everyone else, addressed or not.
        let addressed_them = item("CLR-005", Priority::High, "Security", Some("Zach"));
        assert_eq!(
            evaluate(&addressed_them, &chair(), &stakes()),
            Eligibility::NotEligible
        );
    }

    #[test]
    fn shared_lane_grants_every_group_member_and_only_them() {
        let unaddressed = item("CLR-006", Priority::High, "QA", None);
        assert_eq!(
            evaluate(&unaddressed, &qa_member(), &stakes()),
            Eligibility::SharedOwner
        );
        let addressed_group = item("CLR-007", Priority::High, "QA", Some("QA Guild"));
        assert_eq!(
            evaluate(&addressed_group, &qa_member(), &stakes()),
            Eligibility::ByGroup
        );
        // A fellow non-member of the lane's group loses it outright.
        assert_eq!(
            evaluate(&unaddressed, &chair(), &stakes()),
            Eligibility::NotEligible
        );
        assert_eq!(
            evaluate(&unaddressed, &morgan(), &stakes()),
            Eligibility::NotEligible
        );
    }

    // ---- the veto outranks direct address ---------------------------------

    #[test]
    fn other_holder_veto_outranks_direct_address() {
        // AC1-shaped: Development sole-owned by Alex; Zach's groups exclude
        // Alex. Unassigned AND directly addressed, the item never poses.
        let dev = Stakeholders::new(vec![CategoryOwners::new(
            "Development",
            vec!["Alex".into()],
        )]);
        let unassigned = item("CLR-008", Priority::High, "Development", None);
        assert_eq!(
            evaluate(&unassigned, &chair(), &dev),
            Eligibility::NotEligible
        );
        assert!(
            !eligible_items(std::slice::from_ref(&unassigned), &chair(), &dev)
                .iter()
                .any(|i| i.id == "CLR-008")
        );

        let addressed_zach = item("CLR-009", Priority::High, "Development", Some("Zach"));
        assert_eq!(
            evaluate(&addressed_zach, &chair(), &dev),
            Eligibility::NotEligible
        );
        assert!(
            !eligible_items(std::slice::from_ref(&addressed_zach), &chair(), &dev)
                .iter()
                .any(|i| i.id == "CLR-009")
        );

        // The named owner keeps their lane through the same config.
        let alex = CurrentUser::new("Alex", Vec::new());
        assert_eq!(evaluate(&unassigned, &alex, &dev), Eligibility::SoleOwner);
    }

    // ---- the consolidated D-14 regression matrix (AC-1 … AC-4) --------

    #[test]
    fn routing_law_regression_matrix_pins_four_rules() {
        // AC-1 — the named operator IS the sole owner of Security; their
        // questions arrive on their OWN lane even when unaddressed. A
        // foreign address on the same lane does NOT strip it (addresses
        // are grants of reach-in, never revocation of ownership).
        assert_eq!(
            evaluate(
                &item("M-1", Priority::High, "Security", None),
                &morgan(),
                &stakes()
            ),
            Eligibility::SoleOwner
        );
        assert_eq!(
            evaluate(
                &item("M-1b", Priority::High, "Security", Some("Someone Else")),
                &morgan(),
                &stakes()
            ),
            Eligibility::SoleOwner
        );

        // AC-2 — Development is sole-owned by Alex; the addressed-but-not-
        // owner (Zach, groups excluding Alex) gets NOTHING, addressed or
        // unaddressed: the veto outranks direct address.
        let dev = Stakeholders::new(vec![CategoryOwners::new(
            "Development",
            vec!["Alex".into()],
        )]);
        assert_eq!(
            evaluate(
                &item("M-2", Priority::High, "Development", None),
                &chair(),
                &dev
            ),
            Eligibility::NotEligible
        );
        assert_eq!(
            evaluate(
                &item("M-2b", Priority::High, "Development", Some("Zach")),
                &chair(),
                &dev
            ),
            Eligibility::NotEligible
        );

        // AC-3 — UNOWNED InfoSec: the non-guest chair INHERITS it;
        // the guest seat gets the same view without this item (subset).
        let q = item("M-3", Priority::High, "InfoSec", None);
        assert_eq!(
            evaluate(&q, &chair(), &stakes()),
            Eligibility::SeatInherited
        );
        assert_eq!(evaluate(&q, &guest(), &stakes()), Eligibility::NotEligible);

        // AC-4 — an OWNERLESS category: guests inherit nothing, ever.
        let ux = item("M-4", Priority::High, "UX", None);
        assert_eq!(evaluate(&ux, &guest(), &stakes()), Eligibility::NotEligible);
        assert_eq!(
            recommended_next(std::slice::from_ref(&ux), &guest(), &stakes()),
            None
        );
        let chaired_views: Vec<String> =
            eligible_items(std::slice::from_ref(&ux), &chair(), &stakes())
                .iter()
                .map(|i| i.id.clone())
                .collect();
        assert_eq!(
            chaired_views,
            vec!["M-4"],
            "the seated operator does inherit the unowned lane"
        );
    }

    // ---- rule 4: seat inheritance, guest excluded -------------------------

    #[test]
    fn unowned_lane_is_seat_inherited_by_a_named_chair_but_never_guest() {
        // Present-entry-with-empty-member-list (the seeded-repo form)…
        let q = item("CLR-010", Priority::Blocking, "InfoSec", None);
        assert_eq!(
            evaluate(&q, &chair(), &stakes()),
            Eligibility::SeatInherited
        );
        assert_eq!(evaluate(&q, &guest(), &stakes()), Eligibility::NotEligible);

        // …and the missing-entry form classify IDENTICALLY (Unowned).
        let bare = Stakeholders::new(Vec::new());
        assert_eq!(lane_claim("InfoSec", &chair(), &bare), LaneClaim::Unowned);
        assert_eq!(evaluate(&q, &chair(), &bare), Eligibility::SeatInherited);
        assert_eq!(evaluate(&q, &guest(), &bare), Eligibility::NotEligible);

        // Poseable from the chair's seat: the seat-inherited lane drives the
        // recommendation and the eligible partition.
        assert_eq!(
            recommended_next(std::slice::from_ref(&q), &chair(), &stakes()).map(|r| r.id.as_str()),
            Some("CLR-010")
        );
        let rest: Vec<String> = eligible_items(std::slice::from_ref(&q), &guest(), &stakes())
            .iter()
            .map(|i| i.id.clone())
            .collect();
        assert!(rest.is_empty(), "guest must inherit nothing: {rest:?}");

        // Defensive: an UNSET identity (neither guest-named nor real) is
        // treated like the guest — it inherits nothing.
        let blank = CurrentUser::new("", Vec::new());
        assert_eq!(evaluate(&q, &blank, &stakes()), Eligibility::NotEligible);
    }

    // ---- per-user, per-spell classification --------------------------------

    #[test]
    fn mixed_owner_list_splits_person_and_group_per_user() {
        let mixed = Stakeholders::new(vec![CategoryOwners::new(
            "Security",
            vec!["Morgan".into(), "QA Guild".into()],
        )]);
        let q = item("CLR-011", Priority::High, "Security", None);
        // The person match takes precedence for the named holder…
        assert_eq!(evaluate(&q, &morgan(), &mixed), Eligibility::SoleOwner);
        // …a fellow group member classifies SHARED…
        assert_eq!(evaluate(&q, &qa_member(), &mixed), Eligibility::SharedOwner);
        // …and every other operator stays vetoed.
        assert_eq!(evaluate(&q, &chair(), &mixed), Eligibility::NotEligible);
        assert_eq!(evaluate(&q, &guest(), &mixed), Eligibility::NotEligible);
    }

    #[test]
    fn classification_folds_trailing_spaces_and_case_like_find_does() {
        // Category "infosec " vs entry "InfoSec", owner " ZACH " vs user Zach.
        let drifted =
            Stakeholders::new(vec![CategoryOwners::new("InfoSec", vec!["  ZACH ".into()])]);
        let q = item("CLR-012", Priority::High, "infosec ", None);
        assert_eq!(evaluate(&q, &chair(), &drifted), Eligibility::SoleOwner);
        assert_eq!(evaluate(&q, &morgan(), &drifted), Eligibility::NotEligible);

        // Entry-name drift (hand-edited config heading "InfoSec "): the
        // SAME trim-plus-casefold stance holds on the entry side too, and
        // the empty member list still reads Unowned (seat-inheritable).
        let padded_entry = Stakeholders::new(vec![CategoryOwners::new("InfoSec ", Vec::new())]);
        let q2 = item("CLR-012b", Priority::High, "InfoSec", None);
        assert_eq!(
            evaluate(&q2, &chair(), &padded_entry),
            Eligibility::SeatInherited
        );
        assert_eq!(
            evaluate(&q2, &guest(), &padded_entry),
            Eligibility::NotEligible
        );
    }

    // ---- recommendation ordering -------------------------------------------

    #[test]
    fn recommends_blocking_before_normal_smallest_id_breaks_ties() {
        // All three unowned-lane questions seat-inherit to the chair;
        // Blocking beats Normal and the smallest CLR id breaks the tie.
        let q = vec![
            item("CLR-013", Priority::Normal, "InfoSec", None),
            item("CLR-015", Priority::Blocking, "InfoSec", None),
            item("CLR-014", Priority::Blocking, "InfoSec", None),
        ];
        assert_eq!(
            recommended_next(&q, &chair(), &stakes()).map(|r| r.id.as_str()),
            Some("CLR-014")
        );
        let after: Vec<OpenItem> = q.iter().filter(|i| i.id != "CLR-014").cloned().collect();
        assert_eq!(
            recommended_next(&after, &chair(), &stakes()).map(|r| r.id.as_str()),
            Some("CLR-015")
        );
        let lone = item("CLR-016", Priority::Blocking, "InfoSec", None);
        assert!(
            recommended_next(std::slice::from_ref(&lone), &guest(), &stakes()).is_none(),
            "the guest seat has no lane to recommend from"
        );
    }

    /// Conjunctive acceptance-criterion-2 pin: the ByGroup ELIGIBILITY of a
    /// group-addressed item and the PRIORITY ranking race inside one single
    /// `recommended_next` call for a QA-Guild seat — the group-addressed
    /// High item must win over a lower-priority eligible (seat-inherited)
    /// one.
    #[test]
    fn group_addressed_high_outranks_lower_priority_eligibles_in_recommendation() {
        let pooled = vec![
            item("CLR-020", Priority::Normal, "InfoSec", None), // seat-inherited, lower priority
            item("CLR-021", Priority::High, "QA", Some("QA Guild")), // group-addressed
            item("CLR-022", Priority::Normal, "UX", None),      // seat-inherited, later id
        ];
        let recs = recommended_next(&pooled, &qa_member(), &stakes()).map(|r| r.id.as_str());
        assert_eq!(
            recs,
            Some("CLR-021"),
            "ByGroup eligibility grants the lane AND High ranks it first"
        );
        // The group-addressed item is reachable at all BECAUSE of the group
        // match: strip Robin's membership and the only eligible High item
        // vanishes — the recommendation falls to the seat-inherited Normal.
        let outsider = CurrentUser::new("Robin", Vec::new());
        assert_eq!(
            recommended_next(&pooled, &outsider, &stakes()).map(|r| r.id.as_str()),
            Some("CLR-020"),
        );
    }

    #[test]
    fn ownership_gap_items_are_never_recommended_despite_high_priority() {
        let gap = ownership_item("CLR-017", Priority::Blocking, "InfoSec");
        let q = vec![gap, item("CLR-018", Priority::Normal, "InfoSec", None)];
        assert_eq!(
            recommended_next(&q, &chair(), &stakes()).map(|r| r.id.as_str()),
            Some("CLR-018")
        );
        // They DO stay visible in the eligible partition (paper, not chat).
        assert_eq!(eligible_items(&q, &chair(), &stakes()).len(), 2);
    }

    #[test]
    fn unaddressed_other_lane_address_does_not_leak_across_users() {
        // Robin (QA Guild) inherits InfoSec/UX too — seat inheritance is a
        // property of the SEAT, not of the lane's nominal readers.
        let ux = item("CLR-019", Priority::Normal, "UX", None);
        assert_eq!(
            evaluate(&ux, &qa_member(), &stakes()),
            Eligibility::SeatInherited
        );
        assert_eq!(
            recommended_next(std::slice::from_ref(&ux), &qa_member(), &stakes())
                .map(|r| r.id.as_str()),
            Some("CLR-019")
        );
    }
}

// ---- describe_lanes goldens --------------------------------------------------

#[cfg(test)]
mod digest_goldens {
    use super::*;
    use crate::domain::{CategoryOwners, CurrentUser, GUEST_NAME, Stakeholders};

    fn mixed_config() -> Stakeholders {
        // One sole lane (Infra, for Ada), one group-shared lane (QA, via QA
        // Guild), two unowned lanes (InfoSec, UX) — plus a General entry
        // that must never surface as a "lane".
        Stakeholders::new(vec![
            CategoryOwners::new("General", vec![]),
            CategoryOwners::new("Infra", vec!["Ada".into()]),
            CategoryOwners::new("QA", vec!["QA Guild".into()]),
            CategoryOwners::new("InfoSec", Vec::new()),
            CategoryOwners::new("UX", Vec::new()),
        ])
    }

    #[test]
    fn digest_pinned_lines_for_the_owing_chair() {
        let ada = CurrentUser::new("Ada", vec!["QA Guild".into()]);
        let got = describe_lanes(&ada, &mixed_config());
        assert_eq!(
            got,
            "Sole-owned lanes: Infra\n\
             Shared lanes: QA (via QA Guild)\n\
             Seat-inherited unowned lanes: InfoSec, UX"
        );
    }

    #[test]
    fn digest_marks_the_vetoed_viewpoint_without_listing_foreign_lanes() {
        // Robin shares QA and inherits the unowned pair, but Infra (Ada's
        // sole lane) must NOT appear in Robin's digest.
        let robin = CurrentUser::new("Robin", vec!["QA Guild".into()]);
        let got = describe_lanes(&robin, &mixed_config());
        assert!(!got.contains("Infra"), "{got}");
        assert_eq!(
            got,
            "Shared lanes: QA (via QA Guild)\nSeat-inherited unowned lanes: InfoSec, UX"
        );
    }

    #[test]
    fn digest_empty_for_guest_and_for_empty_config() {
        let ada = CurrentUser::new("Ada", Vec::new());
        assert_eq!(
            describe_lanes(&CurrentUser::new(GUEST_NAME, Vec::new()), &mixed_config()),
            ""
        );
        assert_eq!(describe_lanes(&ada, &Stakeholders::default()), "");
    }

    #[test]
    fn digest_folds_duplicate_spelled_entries_from_hand_edited_configs() {
        let dup = Stakeholders::new(vec![
            CategoryOwners::new("InfoSec", Vec::new()),
            CategoryOwners::new("infosec", Vec::new()), // drifting spelling, same lane
        ]);
        let ada = CurrentUser::new("Ada", Vec::new());
        assert_eq!(
            describe_lanes(&ada, &dup),
            "Seat-inherited unowned lanes: InfoSec"
        );
    }
}
