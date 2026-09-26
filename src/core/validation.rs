//! Central verification of the agent's structured envelope (SPECIFICATION.md
//! §16): EVERYTHING is checked before any file mutates; a single fatal
//! problem rolls the whole turn back (zero artifact writes, no commit).

use std::collections::HashSet;

use crate::core::ids;
use crate::core::routing::{self, Eligibility};
use crate::core::state::PlannerState;
use crate::domain::{Authority, CurrentUser, DecisionBrief, ItemKind, OpenItem, Priority};
use crate::harness::TurnEnvelope;

/// Field-wise patch for an existing item: `Some` = set, `None` = untouched.
/// Clearing an assignment is intentionally unsupported in the MVP.
#[derive(Debug, Clone, Default)]
pub struct UpdatePatch {
    pub priority: Option<Priority>,
    pub authority: Option<Authority>,
    pub kind: Option<ItemKind>,
    pub category: Option<String>,
    pub assigned_to: Option<String>,
    pub question: Option<String>,
    pub reason: Option<String>,
    pub feature_id: Option<String>,
    pub recommendation: Option<String>,
    pub evidence: Option<String>,
    pub decision_brief: Option<DecisionBrief>,
}

#[cfg(test)]
#[path = "validation/decision_tests.rs"]
mod decision_tests;
#[path = "validation/requested_action.rs"]
mod requested_action_validation;

/// Fully-checked outcome of a turn, ready for `core::apply`.
#[derive(Debug, Clone)]
pub struct NormalizedTurn {
    pub assistant_message: String,
    pub change_summary: Option<String>,
    /// Complete replacement specification (verified non-blank).
    pub spec_markdown: Option<String>,
    pub document_updates: Vec<(String, String)>,
    /// Application-owned planning records included in the same artifact transaction.
    pub additional_planning_artifacts: Vec<(String, String)>,
    pub added: Vec<OpenItem>,
    pub updates: Vec<(String, UpdatePatch)>,
    pub resolved: Vec<String>,
    pub next_question_id: Option<String>,
    /// Model-interpreted application intent, dispatched only after this
    /// turn passes validation and is adopted by the Main Chat.
    pub requested_action: Option<crate::harness::RequestedAction>,
    /// Non-fatal observations shown to the user (e.g. why next-question was
    /// dropped — routing sovereignty, §18).
    pub warnings: Vec<String>,
    pub workflow: Option<crate::core::workflow::Workflow>,
    pub task_batch: Option<crate::core::workflow::TaskBatch>,
}

const QUESTION_CHAR_CAP: usize = 2000;
const SUMMARY_CHAR_CAP: usize = 60;

pub fn validate(
    envelope: &TurnEnvelope,
    state: &PlannerState,
    user: &CurrentUser,
) -> Result<NormalizedTurn, Vec<String>> {
    validate_for_turn(
        envelope,
        state,
        user,
        crate::core::workflow::TurnPurpose::Interview,
    )
}

pub fn validate_for_turn(
    envelope: &TurnEnvelope,
    state: &PlannerState,
    user: &CurrentUser,
    purpose: crate::core::workflow::TurnPurpose,
) -> Result<NormalizedTurn, Vec<String>> {
    let mut fatals: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let requested_action = match requested_action_validation::validate(envelope, purpose) {
        Ok(action) => action,
        Err(problems) => {
            fatals.extend(problems);
            None
        }
    };

    if let Some(v) = envelope.schema_version
        && v != 2
    {
        fatals.push(format!(
            "unsupported schema_version {v} after normalization (expected 2)"
        ));
    }
    if envelope.assistant().trim().is_empty() {
        fatals.push("assistant_message is empty".into());
    }

    // ---- specification -----------------------------------------------------
    let spec_markdown = match envelope.updated_spec() {
        None => None,
        Some(raw) if raw.trim().is_empty() => {
            fatals.push("updated_specification is blank — send the FULL document or null".into());
            None
        }
        Some(raw) => {
            if state
                .spec_text
                .as_deref()
                .is_some_and(|cur| cur.trim_end() == raw.trim_end())
            {
                None // cosmetic no-change
            } else {
                if let Err(problem) = crate::core::specification::validate_layout(raw) {
                    fatals.push(problem);
                }
                Some(raw.to_string())
            }
        }
    };

    let modular = crate::artifacts::product_docs::load_modules(&state.repo_root)
        .map(|parts| parts.is_some())
        .unwrap_or_else(|error| {
            fatals.push(format!("product modules unreadable: {error}"));
            false
        });
    if modular && spec_markdown.is_some() {
        fatals.push(
            "updated_specification is retired for modular products; use document_updates".into(),
        );
    }
    if !modular
        && envelope
            .document_updates
            .as_ref()
            .is_some_and(|updates| !updates.is_empty())
    {
        fatals.push("document_updates require product-module migration".into());
    }
    if spec_markdown.is_some()
        && envelope
            .document_updates
            .as_ref()
            .is_some_and(|updates| !updates.is_empty())
    {
        fatals.push("updated_specification and document_updates cannot be combined".into());
    }
    let mut document_updates = Vec::new();
    let mut seen_documents = HashSet::new();
    for update in envelope.document_updates.as_deref().unwrap_or_default() {
        if !seen_documents.insert(update.document_id.as_str()) {
            fatals.push(format!("duplicate document_id {}", update.document_id));
            continue;
        }
        let path = match crate::artifacts::product_docs::document_path_for_update(
            &state.repo_root,
            &update.document_id,
            &update.content,
        ) {
            Ok(path) => path,
            Err(error) => {
                fatals.push(error.to_string());
                continue;
            }
        };
        if update.content.trim().is_empty() {
            fatals.push(format!("{}: content must not be blank", update.document_id));
            continue;
        }
        if update.document_id.starts_with("product:") {
            if let Err(error) = crate::artifacts::product_docs::validate_module(&update.content) {
                fatals.push(format!("{}: {error}", update.document_id));
            }
            if let Ok(old) = std::fs::read_to_string(&path)
                && let Err(error) =
                    crate::artifacts::product_docs::preserved_ids(&old, &update.content)
            {
                fatals.push(format!("{}: {error}", update.document_id));
            }
        }
        if let Some(id) = update.document_id.strip_prefix("feature:")
            && let Err(error) = crate::core::specification::validate_feature(id, &update.content)
        {
            fatals.push(format!("{}: {error}", update.document_id));
        }
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => {}
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound
                    && (update.document_id.starts_with("feature:")
                        || update.document_id.starts_with("product:")) => {}
            _ => fatals.push(format!(
                "{}: target must be an approved regular document",
                update.document_id
            )),
        }
        document_updates.push((update.document_id.clone(), update.content.clone()));
    }

    // ---- resolutions --------------------------------------------------------
    let existing: HashSet<&str> = state.items.iter().map(|i| i.id.as_str()).collect();
    let mut resolved: Vec<String> = Vec::new();
    for rid in envelope.resolved() {
        if !existing.contains(rid.as_str()) {
            fatals.push(format!("open_items_resolved references unknown id “{rid}”"));
            continue;
        }
        if !resolved.iter().any(|r| r == rid) {
            resolved.push(rid.clone());
        }
    }

    // ---- updates --------------------------------------------------------------
    let mut updates: Vec<(String, UpdatePatch)> = Vec::new();
    for u in envelope.updated() {
        let uid = match &u.id {
            Some(id) if !id.trim().is_empty() => id.clone(),
            _ => {
                fatals.push("open_items_updated entry missing id".into());
                continue;
            }
        };
        if !existing.contains(uid.as_str()) {
            fatals.push(format!("open_items_updated references unknown id “{uid}”"));
            continue;
        }
        if envelope.resolved().iter().any(|r| r == &uid) {
            fatals.push(format!(
                "“{uid}”: cannot update and resolve the same item in one turn"
            ));
            continue;
        }
        let mut patch = UpdatePatch::default();
        if let Some(raw) = u.priority.as_deref().filter(|s| !s.trim().is_empty()) {
            match Priority::parse_i(raw) {
                Some(p) => patch.priority = Some(p),
                None => fatals.push(format!("“{uid}”: unrecognized priority “{raw}”")),
            }
        }
        if let Some(raw) = u.authority.as_deref() {
            match Authority::parse_i(raw) {
                Some(authority)
                    if state
                        .items
                        .iter()
                        .any(|i| i.id == uid && i.authority == Authority::Human)
                        && authority != Authority::Human =>
                {
                    fatals.push(format!(
                        "{uid}: Human authority cannot be downgraded by the agent"
                    ))
                }
                Some(authority) => patch.authority = Some(authority),
                None => fatals.push(format!("{uid}: invalid authority {raw}")),
            }
        }
        if let Some(raw) = u.kind.as_deref().filter(|s| !s.trim().is_empty()) {
            match ItemKind::parse_i(raw) {
                Some(k) => patch.kind = Some(k),
                None => fatals.push(format!("“{uid}”: unrecognized kind “{raw}”")),
            }
        }
        match u.category.as_deref() {
            None => {}
            Some(raw) if raw.trim().is_empty() => {
                fatals.push(format!("“{}”: category cleared to empty", uid))
            }
            Some(raw) => patch.category = Some(raw.trim().to_string()),
        }
        if let Some(raw) = u.assigned_to.as_deref().filter(|s| !s.trim().is_empty()) {
            patch.assigned_to = Some(raw.trim().to_string());
        }
        if let Some(raw) = u.question.as_deref().filter(|s| !s.trim().is_empty()) {
            if raw.trim().chars().count() > QUESTION_CHAR_CAP {
                fatals.push(format!(
                    "“{}”: clarifying question longer than {QUESTION_CHAR_CAP} chars",
                    uid
                ));
            } else {
                patch.question = Some(raw.trim().to_string());
            }
        }
        if let Some(raw) = u.reason.as_deref().filter(|s| !s.trim().is_empty()) {
            patch.reason = Some(raw.trim().to_string());
        }
        if let Some(id) = u.feature_id.as_deref() {
            if valid_feature_reference(state, envelope, id) {
                patch.feature_id = Some(id.to_string());
            } else {
                fatals.push(format!("{uid}: invalid feature reference {id}"));
            }
        }
        if let Some(value) = &u.recommendation {
            patch.recommendation = Some(value.trim().to_string());
        }
        if let Some(value) = &u.evidence {
            patch.evidence = Some(value.trim().to_string());
        }
        if let Some(brief) = &u.decision_brief {
            let mut brief = brief.clone();
            match brief.bind_to_item(&uid) {
                Ok(()) if brief.adr_assessment.is_some() => {
                    patch.decision_brief = Some(brief)
                }
                Ok(()) => fatals.push(format!(
                    "{uid}: decision brief must assess whether an approved choice needs a durable decision record"
                )),
                Err(error) => fatals.push(format!("{uid}: {error}")),
            }
        }
        updates.push((uid.clone(), patch));
    }

    // ---- additions -------------------------------------------------------
    // Burned forever: everything ever issued — existing queue + this turn's
    // resolutions (numbers are retired, never recycled).
    let mut burned: HashSet<String> = state.items.iter().map(|i| i.id.clone()).collect();
    burned.extend(state.resolved_items.iter().map(|i| i.id.clone()));
    burned.extend(resolved.iter().cloned());
    let mut added: Vec<OpenItem> = Vec::new();
    for a in envelope.added() {
        let tag = a.id.as_deref().unwrap_or("<unnumbered>");
        let priority = match Priority::parse_i(a.priority.as_deref().unwrap_or("")) {
            Some(p) => p,
            None => {
                fatals.push(format!(
                    "new item {tag}: invalid priority “{}”",
                    a.priority.as_deref().unwrap_or("")
                ));
                continue;
            }
        };
        let kind = match ItemKind::parse_i(a.kind.as_deref().unwrap_or("")) {
            Some(k) => k,
            None => {
                fatals.push(format!(
                    "new item {tag}: invalid kind “{}”",
                    a.kind.as_deref().unwrap_or("")
                ));
                continue;
            }
        };
        let category = a
            .category
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let assigned = a
            .assigned_to
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let question = a
            .question
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        match (category, assigned, question) {
            (Some(cat), Some(to), Some(question)) => {
                let id = match a.id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                    None => {
                        let id = ids::next_free(burned.iter().cloned());
                        // Reserve the freshly minted id so later unnumbered
                        // items in the SAME batch keep numbering monotonic.
                        burned.insert(id.clone());
                        id
                    }
                    Some(want) if !ids::is_valid_id(want) => {
                        fatals.push(format!("new item {tag}: malformed requested id “{want}”"));
                        continue;
                    }
                    Some(want) if burned.contains(want) => {
                        fatals.push(format!("new item {tag}: requested id “{want}” is already taken"));
                        continue;
                    }
                    Some(want) => {
                        burned.insert(want.to_string());
                        want.to_string()
                    }
                };
                let authority = match a.authority.as_deref() {
                    Some(raw) => match Authority::parse_i(raw) {
                        Some(value) => value,
                        None => { fatals.push(format!("new item {tag}: invalid authority {raw}")); continue; }
                    },
                    None => Authority::Human,
                };
                let mut item = OpenItem::new(
                    id,
                    priority,
                    kind,
                    cat.to_string(),
                    Some(to.to_string()),
                    question.to_string(),
                    a.reason
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .unwrap_or_default(),
                );
                item.authority = authority;
                if let Some(id) = a.feature_id.as_deref() {
                    if valid_feature_reference(state, envelope, id) { item.feature_id = Some(id.to_string()); }
                    else { fatals.push(format!("new item {tag}: invalid feature reference {id}")); }
                }
                item.recommendation = a.recommendation.as_deref().unwrap_or("").trim().to_string();
                item.evidence = a.evidence.as_deref().unwrap_or("").trim().to_string();
                if let Some(brief) = &a.decision_brief {
                    let mut brief = brief.clone();
                    match brief.bind_to_item(&item.id) {
                        Ok(()) if brief.adr_assessment.is_some() => {
                            item.decision_brief = Some(brief)
                        }
                        Ok(()) => fatals.push(format!(
                            "new item {tag}: decision brief must assess whether an approved choice needs a durable decision record"
                        )),
                        Err(error) => fatals.push(format!("new item {tag}: {error}")),
                    }
                }
                if item.decision_brief.is_some() && authority == Authority::Agent {
                    fatals.push(format!("new item {tag}: decision briefs need Human or Review authority"));
                }
                if authority == Authority::Review
                    && item.recommendation.is_empty()
                    && item
                        .decision_brief
                        .as_ref()
                        .is_none_or(|brief| brief.recommendation.is_none())
                {
                    fatals.push(format!("new review item {tag} requires a provisional recommendation"));
                }
                added.push(item);
            }
            _ => fatals.push(format!(
                "new item {tag}: category, assigned_to and question are all required (got category={:?}, assigned_to={:?}, question={:?})",
                a.category, a.assigned_to, a.question
            )),
        }
    }

    // ---- next question: the D-14 routing law is decided HERE, not by the
    //         agent (§8, §18). A misrouted id is a FATAL problem — the whole
    //         turn is rejected with zero mutation. Unknown/resolved/ownership
    //         ids are noise, not law violations: they cost a warning only.
    let mut next_question_id = None;
    if let Some(nqid) = envelope.next_question_id.as_deref() {
        match state.items.iter().find(|i| i.id == nqid) {
            None => warnings.push(format!("next question “{nqid}” dropped: unknown or just-resolved id")),
            Some(item) if resolved.iter().any(|r| r == &item.id) => {
                warnings.push(format!("next question “{}” dropped: it is being resolved this turn", item.id));
            }
            Some(item) if item.kind == ItemKind::Ownership => warnings.push(format!(
                "next question “{}” dropped: ownership gaps are handled in the Settings panel, not asked in chat",
                item.id
            )),
            Some(item) => match routing::evaluate(item, user, &state.config.stakeholders) {
                Eligibility::NotEligible => fatals.push(format!(
                    "next question “{}” (category “{}”, assignee “{}”) is not poseable to the seated user “{}” — it violates the routing law, so the ENTIRE turn is rejected and nothing is saved",
                    item.id,
                    item.category,
                    item.assigned_to.as_deref().unwrap_or("unassigned"),
                    user.name
                )),
                _ if item.authority == Authority::Human && item.priority == Priority::Blocking => next_question_id = Some(item.id.clone()),
                _ => warnings.push(format!("next question {} dropped: only Human/Blocking items may interrupt chat", item.id)),
            },
        }
    }

    let change_summary = envelope
        .change_summary
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.chars().take(SUMMARY_CHAR_CAP).collect::<String>());

    if !fatals.is_empty() {
        return Err(fatals);
    }
    let mut normalized = NormalizedTurn {
        assistant_message: envelope.assistant().trim().to_string(),
        change_summary,
        spec_markdown,
        document_updates,
        additional_planning_artifacts: Vec::new(),
        added,
        updates,
        resolved,
        next_question_id,
        requested_action,
        warnings,
        workflow: None,
        task_batch: None,
    };
    crate::core::workflow::prepare(state, envelope, &mut normalized, purpose)?;
    Ok(normalized)
}

fn valid_feature_reference(state: &PlannerState, envelope: &TurnEnvelope, id: &str) -> bool {
    if !crate::artifacts::product_docs::valid_feature_id(id) {
        return false;
    }
    crate::artifacts::product_docs::document_path(&state.repo_root, &format!("feature:{id}"))
        .is_ok()
        || envelope
            .document_updates
            .as_deref()
            .unwrap_or_default()
            .iter()
            .any(|update| update.document_id == format!("feature:{id}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{TurnEnvelope, TurnItem, TurnItemUpdate};

    // Monotonic suffix keeps every sandbox UNIQUE even when two tests happen
    // to carry equally-sized item lists (parallel-suite hygiene).
    static VAL_SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    fn base_state(items: Vec<OpenItem>) -> PlannerState {
        let seq = VAL_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("packet_val_{seq}_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&root);
        let mut st = PlannerState::load(&root).expect("temp state loads");
        st.items = items;
        st
    }

    fn item(id: &str, kind: ItemKind, cat: &str, to: &str) -> OpenItem {
        OpenItem::new(
            id.into(),
            Priority::Normal,
            kind,
            cat.into(),
            Some(to.into()),
            "question text".into(),
            "it matters".into(),
        )
    }

    /// Stand-in for a genuinely unassigned item: an address no user can
    /// match, so the lane's own claim decides eligibility.
    fn unassigned(id: &str, kind: ItemKind, cat: &str) -> OpenItem {
        let mut o = item(id, kind, cat, "Unassigned");
        o.assigned_to = None;
        o
    }

    fn env(next: Option<&str>) -> TurnEnvelope {
        TurnEnvelope {
            schema_version: Some(2),
            assistant_message: Some("Sure — recorded.".into()),
            change_summary: None,
            document_updates: None,
            updated_specification: None,
            open_items_added: None,
            open_items_updated: None,
            open_items_resolved: None,
            next_question_id: next.map(Into::into),
            interview: None,
            task_stories: None,
            requested_action: None,
            task_outline: None,
        }
    }

    #[test]
    fn unnumbered_adds_in_one_batch_number_monotonically_without_collisions() {
        let st = base_state(Vec::new());
        let mut env = env(None);
        env.open_items_added = Some(vec![
            TurnItem {
                authority: None,
                id: None,
                kind: Some("Question".into()),
                category: Some("QA".into()),
                assigned_to: Some("QA Guild".into()),
                priority: Some("High".into()),
                question: Some("first unnumbered?".into()),
                reason: None,
                resolution_note: None,
                feature_id: None,
                recommendation: None,
                evidence: None,
                decision_brief: None,
            },
            TurnItem {
                authority: None,
                id: None,
                kind: Some("Question".into()),
                category: Some("QA".into()),
                assigned_to: Some("QA Guild".into()),
                priority: Some("High".into()),
                question: Some("second unnumbered?".into()),
                reason: None,
                resolution_note: None,
                feature_id: None,
                recommendation: None,
                evidence: None,
                decision_brief: None,
            },
        ]);
        let v = validate(
            &env,
            &st,
            &CurrentUser::new("Zach", vec!["QA Guild".into()]),
        )
        .unwrap();
        let ids: Vec<&str> = v.added.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["CLR-001", "CLR-002"],
            "auto-numbered siblings must reserve each minted id"
        );
    }

    /// The next-question arm of the D-14 law: deny-by-veto is FATAL (the
    /// whole turn rolls back); grant-by-group, grant-by-unowned-seat and
    /// General-always preserve the id; unknown/resolved/ownership ids
    /// degrade to warnings exactly as pinned before.
    #[test]
    fn next_question_enforces_the_routing_law() {
        use crate::domain::{CategoryOwners, Stakeholders};
        let zach = CurrentUser::new("Zach", vec!["QA Guild".into()]);
        let mut st = base_state(vec![
            item("CLR-001", ItemKind::Question, "Security", "Security Team"),
            item("CLR-002", ItemKind::Question, "QA", "QA Guild"),
            unassigned("CLR-003", ItemKind::Question, "InfoSec"),
            item("CLR-004", ItemKind::Question, "General", "Everyone"),
        ]);
        for item in &mut st.items {
            item.priority = Priority::Blocking;
        }
        // Security is sole-owned by Priya (veto vs Zach); QA is shared
        // through QA Guild (Zach's); InfoSec has an entry with NO members
        // (unowned → seat-inherited by Zach, the non-guest seat).
        st.config.stakeholders = Stakeholders::new(vec![
            CategoryOwners::new("Security", vec!["Priya".into()]),
            CategoryOwners::new("QA", vec!["QA Guild".into()]),
            CategoryOwners::new("InfoSec", Vec::new()),
        ]);

        // Deny-by-veto: even an unambiguous Security question is a FATAL
        // routing-law problem that rejects the whole turn.
        let vetoed = validate(&env(Some("CLR-001")), &st, &zach).unwrap_err();
        assert!(
            vetoed
                .iter()
                .any(|f| f.contains("CLR-001") && f.contains("violates the routing law")),
            "expected the veto fatal, got: {vetoed:?}"
        );
        assert!(
            vetoed
                .iter()
                .any(|f| f.contains('“') && f.contains("Zach") && f.contains('”')),
            "the fatal must name the seated user in the message shape: {vetoed:?}"
        );

        // Grant-by-group: a QA question addressed to Zach's group is obeyed.
        let grouped = validate(&env(Some("CLR-002")), &st, &zach).unwrap();
        assert_eq!(grouped.next_question_id.as_deref(), Some("CLR-002"));

        // Grant-by-unowned-seat: the ownerless InfoSec lane seat-inherits.
        let inherited = validate(&env(Some("CLR-003")), &st, &zach).unwrap();
        assert_eq!(inherited.next_question_id.as_deref(), Some("CLR-003"));

        // General is always poseable.
        let general_route = validate(&env(Some("CLR-004")), &st, &zach).unwrap();
        assert_eq!(general_route.next_question_id.as_deref(), Some("CLR-004"));

        // Warnings-only arms are unchanged: unknown id …
        let unknown = validate(&env(Some("CLR-999")), &st, &zach).unwrap();
        assert_eq!(unknown.next_question_id, None);
        assert!(
            unknown
                .warnings
                .iter()
                .any(|w| w.contains("CLR-999") && w.contains("dropped"))
        );
        // … an id being resolved this turn …
        let mut er = env(Some("CLR-004"));
        er.open_items_resolved = Some(vec!["CLR-004".into()]);
        let mid_resolve = validate(&er, &st, &zach).unwrap();
        assert_eq!(mid_resolve.next_question_id, None);
        assert!(
            mid_resolve
                .warnings
                .iter()
                .any(|w| w.contains("being resolved"))
        );
        // … and ownership-kind ids (handled in the settings panel).
        let mut st2 = st.clone();
        let own = crate::domain::OpenItem::new(
            "CLR-005".into(),
            Priority::Normal,
            ItemKind::Ownership,
            "InfoSec".into(),
            None,
            "nominate an owner".into(),
            "formal nomination".into(),
        );
        st2.items.push(own);
        let ownership_drop = validate(&env(Some("CLR-005")), &st2, &zach).unwrap();
        assert_eq!(ownership_drop.next_question_id, None);
        assert!(
            ownership_drop
                .warnings
                .iter()
                .any(|w| w.contains("Settings panel"))
        );
    }

    #[test]
    fn ownership_items_are_never_chat_questions() {
        let ops = CurrentUser::new("Rita", vec!["Operations".into()]);
        let st = base_state(vec![item(
            "CLR-001",
            ItemKind::Ownership,
            "Operations",
            "Operations Team",
        )]);
        let v = validate(&env(Some("CLR-001")), &st, &ops).unwrap();
        assert_eq!(v.next_question_id, None);
    }

    #[test]
    fn unknown_resolve_is_fatal_and_blocks_everything() {
        let u = CurrentUser::new("Zach", vec![]);
        let st = base_state(vec![item("CLR-001", ItemKind::Question, "General", "All")]);
        let mut e = env(None);
        e.open_items_resolved = Some(vec!["CLR-999".into()]);
        e.updated_specification = Some("# spec\n".into());
        let errs = validate(&e, &st, &u).unwrap_err();
        assert!(errs.iter().any(|f| f.contains("CLR-999")));
    }

    #[test]
    fn fresh_items_get_numbers_after_max_keeps_history_stable() {
        let u = CurrentUser::new("Zach", vec![]);
        let st = base_state(vec![
            item("CLR-001", ItemKind::Question, "General", "All"),
            item("CLR-003", ItemKind::Question, "General", "All"),
        ]);
        let mut e = env(None);
        e.open_items_added = Some(vec![
            TurnItem {
                authority: None,
                id: None,
                kind: Some("Question".into()),
                category: Some("Product".into()),
                assigned_to: Some("Product Owner".into()),
                priority: Some("High".into()),
                question: Some("Which auth flow?".into()),
                resolution_note: None,
                feature_id: None,
                recommendation: None,
                evidence: None,
                decision_brief: None,
                reason: Some("login scoping depends on it".into()),
            },
            TurnItem {
                authority: None,
                id: Some("CLR-002".into()),
                kind: Some("Assumption".into()),
                category: Some("Architecture".into()),
                assigned_to: Some("Alice".into()),
                priority: Some("Normal".into()),
                question: Some("Is SQLite acceptable for parity?".into()),
                resolution_note: None,
                feature_id: None,
                recommendation: None,
                evidence: None,
                decision_brief: None,
                reason: None,
            },
        ]);
        let v = validate(&e, &st, &u).unwrap();
        assert_eq!(v.added.len(), 2);
        assert!(v.added.iter().any(|i| i.id == "CLR-004")); // appended after max
        assert!(v.added.iter().any(|i| i.id == "CLR-002")); // explicit fill honored
    }

    #[test]
    fn duplicate_request_collide_rejects_second() {
        let u = CurrentUser::new("Zach", vec![]);
        let st = base_state(vec![item("CLR-001", ItemKind::Question, "General", "All")]);
        let mut e = env(None);
        let mk = || TurnItem {
            authority: None,
            id: Some("CLR-005".into()),
            kind: Some("Question".into()),
            category: Some("General".into()),
            assigned_to: Some("All".into()),
            priority: Some("Normal".into()),
            question: Some("?".into()),
            resolution_note: None,
            feature_id: None,
            recommendation: None,
            evidence: None,
            decision_brief: None,
            reason: None,
        };
        e.open_items_added = Some(vec![mk(), mk()]);
        let errs = validate(&e, &st, &u).unwrap_err();
        assert!(errs.iter().any(|f| f.contains("already taken")));
    }

    #[test]
    fn blank_spec_fails_the_whole_turn() {
        let u = CurrentUser::new("Zach", vec![]);
        let st = base_state(vec![]);
        let mut e = env(None);
        e.updated_specification = Some("   \n".into());
        let errs = validate(&e, &st, &u).unwrap_err();
        assert!(errs.iter().any(|f| f.contains("updated_specification")));
    }

    #[test]
    fn update_can_change_priority_kind_category_fields() {
        let u = CurrentUser::new("Zach", vec![]);
        let st = base_state(vec![item(
            "CLR-001",
            ItemKind::Assumption,
            "General",
            "All",
        )]);
        let mut e = env(None);
        e.open_items_updated = Some(vec![TurnItemUpdate {
            authority: None,
            id: Some("CLR-001".into()),
            priority: Some("Blocking".into()),
            kind: Some("Question".into()),
            category: Some("QA".into()),
            assigned_to: None,
            question: None,
            reason: Some("regression risk surfaced".into()),
            feature_id: None,
            recommendation: None,
            evidence: None,
            decision_brief: None,
        }]);
        let v = validate(&e, &st, &u).unwrap();
        assert_eq!(v.updates.len(), 1);
        let (_id, patch) = &v.updates[0];
        assert_eq!(patch.priority, Some(Priority::Blocking));
        assert_eq!(patch.kind, Some(ItemKind::Question));
        assert_eq!(patch.category.as_deref(), Some("QA"));
        assert!(patch.assigned_to.is_none());
    }

    // ── Option-1 boundary battery (ticket 004): fatal-class repeatability ──

    /// Table over the fatal classes this verifier owns, proving the
    /// machine planes are PERSONA-INDEPENDENT BY CONSTRUCTION: the
    /// validator's entire input surface is (envelope, state, seated user,
    /// purpose) — no channel carries instruction-channel (persona) text
    /// into this plane. Each bad envelope is validated TWICE against the
    /// SAME git-less, environment-independent state: the pinned fatal
    /// fragment fires and the problem vector is byte-stable across
    /// invocations. If a future refactor ever threads persona bytes (or
    /// instruction-channel text generally) into validation, these pins —
    /// and the sibling cross-file identity asserts elsewhere in the suite
    /// — are the tripwire.
    #[test]
    fn fatal_classes_fire_twice_with_pinned_fragments_and_stable_vectors() {
        use crate::core::workflow::TurnPurpose;
        use crate::domain::{CategoryOwners, Stakeholders};

        // Seated identity: a NON-MEMBER human seat, matching the routing
        // veto case; the other cases are seat-insensitive.
        let seated = CurrentUser::new("Packet Test", Vec::new());

        /// Validates (envelope, state) twice and asserts: (1) the pinned
        /// fragment fired, (2) repetition is byte-stable (deterministic),
        /// (3) the verdict stayed REJECTED both times.
        fn pin(
            user: &CurrentUser,
            label: &str,
            fragment: &str,
            build: impl Fn() -> (PlannerState, TurnEnvelope),
        ) {
            let (st, en) = build();
            let first = validate_for_turn(&en, &st, user, TurnPurpose::Interview).unwrap_err();
            assert!(
                first.iter().any(|f| f.contains(fragment)),
                "{label}: the pinned fatal fragment {fragment:?} did not fire; actual: {first:?}"
            );
            let second = validate_for_turn(&en, &st, user, TurnPurpose::Interview).unwrap_err();
            assert_eq!(
                second, first,
                "{label}: the verifier must be deterministic — repeated calls on the same \
                 inputs must produce byte-identical problem vectors"
            );
        }

        // 1. Routing veto: the seated non-member is aimed at a question in
        //    a lane sole-owned by someone else.
        pin(&seated, "routing-veto", "violates the routing law", || {
            let mut hi = item("CLR-001", ItemKind::Question, "Security", "Priya");
            hi.priority = Priority::Blocking;
            let mut st = base_state(vec![hi]);
            st.config.stakeholders = Stakeholders::new(vec![
                CategoryOwners::new("Security", vec!["Priya".into()]),
                CategoryOwners::new("InfoSec", Vec::new()),
            ]);
            (st, env(Some("CLR-001")))
        });

        // 2. Human-authority demotion attempt: Agent cannot lower a Human item.
        pin(
            &seated,
            "human-downgrade-attempt",
            "cannot be downgraded",
            || {
                let st = base_state(vec![item("CLR-001", ItemKind::Question, "General", "All")]);
                let mut e = env(None);
                e.open_items_updated = Some(vec![TurnItemUpdate {
                    id: Some("CLR-001".into()),
                    authority: Some("Agent".into()),
                    priority: None,
                    kind: None,
                    category: None,
                    assigned_to: None,
                    question: None,
                    reason: None,
                    feature_id: None,
                    recommendation: None,
                    evidence: None,
                    decision_brief: None,
                }]);
                (st, e)
            },
        );

        // 3. Resolve referencing an unknown id.
        pin(
            &seated,
            "resolve-unknown-id",
            "references unknown id",
            || {
                let mut e = env(None);
                e.open_items_resolved = Some(vec!["CLR-999".into()]);
                (
                    base_state(vec![unassigned("CLR-001", ItemKind::Question, "General")]),
                    e,
                )
            },
        );

        // 4. Blank updated_specification.
        pin(&seated, "blank-updated-specification", "is blank", || {
            let mut e = env(None);
            e.updated_specification = Some("   \n  ".into());
            (base_state(Vec::new()), e)
        });

        // 5. Unsupported normalized schema_version (wire versions are handled by the decoder).
        pin(
            &seated,
            "unsupported-schema-version",
            "unsupported schema_version",
            || {
                let mut e = env(None);
                e.schema_version = Some(3);
                (base_state(Vec::new()), e)
            },
        );

        // 6. New Review-kind item lacking a provisional recommendation.
        pin(
            &seated,
            "review-without-recommendation",
            "requires a provisional recommendation",
            || {
                let mut e = env(None);
                e.open_items_added = Some(vec![TurnItem {
                    authority: Some("Review".into()),
                    id: Some("CLR-007".into()),
                    kind: Some("Question".into()),
                    category: Some("Security".into()),
                    assigned_to: Some("Priya".into()),
                    priority: Some("Normal".into()),
                    question: Some("recommend the cipher suite".into()),
                    reason: Some("crypto selection".into()),
                    resolution_note: None,
                    feature_id: None,
                    recommendation: None,
                    evidence: None,
                    decision_brief: None,
                }]);
                (base_state(Vec::new()), e)
            },
        );
    }
}
