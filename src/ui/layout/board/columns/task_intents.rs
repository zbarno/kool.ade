use super::*;
mod linked;
pub(super) use linked::{approval_owned, clarification_owned, review_owned};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
struct Group<'a> {
    root: Option<&'a crate::core::planning_work::Work>,
    planning: Vec<&'a crate::core::planning_work::Work>,
    tasks: Vec<&'a crate::artifacts::task_docs::TaskDocument>,
    items: Vec<&'a crate::domain::item::OpenItem>,
    pr_work: Vec<&'a crate::ui::planning_board::LifecyclePrBoardTask>,
    approvals: Vec<crate::ui::feature_approval::Action>,
    clarifications: Vec<&'a crate::core::planning::QuestionV1>,
    plan_reviews: Vec<&'a crate::core::lifecycle::FeatureGoal>,
}

fn collect_groups<'a>(
    board: &'a crate::ui::planning_board::ViewModel,
    planning: &[&'a crate::core::planning_work::Work],
    tasks: &[&'a crate::artifacts::task_docs::TaskDocument],
    items: &[&'a crate::domain::item::OpenItem],
) -> BTreeMap<String, Group<'a>> {
    let mut groups = BTreeMap::<String, Group<'_>>::new();
    for work in planning {
        let Some(uid) = work.task_intent_root_uid().filter(|uid| *uid != work.uid) else {
            continue;
        };
        groups
            .entry(uid.to_owned())
            .or_default()
            .planning
            .push(work);
    }
    for task in tasks {
        let Some(uid) = crate::artifacts::task_docs::task_intent_uid_for_document(
            task,
            &board.task_documents,
            &board.planning_work,
        ) else {
            continue;
        };
        groups.entry(uid).or_default().tasks.push(task);
    }
    for item in items {
        let Some(uid) = item.task_intent_uid.as_deref() else {
            continue;
        };
        groups.entry(uid.to_owned()).or_default().items.push(item);
    }
    groups.retain(|uid, group| {
        let mut roots = board
            .planning_work
            .iter()
            .filter(|work| work.uid == *uid && work.task_intent_root_uid() == Some(uid.as_str()));
        group.root = match (roots.next(), roots.next()) {
            (Some(root), None) => Some(root),
            _ => None,
        };
        group.root.is_some()
            && (!group.planning.is_empty() || !group.tasks.is_empty() || !group.items.is_empty())
    });
    groups
}

fn add_root_lane_groups<'a>(
    _board: &'a crate::ui::planning_board::ViewModel,
    planning: &[&'a crate::core::planning_work::Work],
    groups: &mut BTreeMap<String, Group<'a>>,
    _items: &[&'a crate::domain::item::OpenItem],
) {
    for root in planning
        .iter()
        .copied()
        .filter(|work| work.task_intent_root_uid() == Some(work.uid.as_str()))
    {
        let group = groups.entry(root.uid.clone()).or_default();
        group.root = Some(root);
        // The request's own planning step is work too; the container header
        // must not consume its card without painting it.
        group.planning.push(root);
    }
}

pub(super) fn pr_owner<'a>(
    board: &'a crate::ui::planning_board::ViewModel,
    task: &crate::ui::planning_board::LifecyclePrBoardTask,
) -> Option<&'a crate::core::planning_work::Work> {
    let document = board
        .task_documents
        .iter()
        .find(|doc| doc.path == task.parent_path)?;
    let uid = crate::artifacts::task_docs::task_intent_uid_for_document(
        document,
        &board.task_documents,
        &board.planning_work,
    )?;
    let mut roots = board
        .planning_work
        .iter()
        .filter(|work| work.uid == uid && work.task_intent_root_uid() == Some(uid.as_str()));
    let root = roots.next()?;
    roots.next().is_none().then_some(root)
}

#[cfg(test)]
fn child_count(board: &crate::ui::planning_board::ViewModel, uid: &str) -> usize {
    board
        .planning_work
        .iter()
        .filter(|work| work.uid != uid && work.task_intent_uid.as_deref() == Some(uid))
        .count()
        + board
            .task_documents
            .iter()
            .filter(|task| {
                crate::artifacts::task_docs::task_intent_uid_for_document(
                    task,
                    &board.task_documents,
                    &board.planning_work,
                )
                .as_deref()
                    == Some(uid)
            })
            .count()
        + board
            .planning_items
            .iter()
            .filter(|item| item.task_intent_uid.as_deref() == Some(uid))
            .count()
}

/// Paint one collapsible view of each TaskIntent represented in this lane.
/// Returns child identities consumed here so callers can render other cards
/// through their normal paths.
pub(super) struct Lane<'a> {
    pub surface: &'a mut dyn Surface,
    pub board: &'a crate::ui::planning_board::ViewModel,
    pub planning: &'a [&'a crate::core::planning_work::Work],
    pub tasks: &'a [&'a crate::artifacts::task_docs::TaskDocument],
    pub items: &'a [&'a crate::domain::item::OpenItem],
    pub column: usize,
    pub selected_path: &'a mut Option<String>,
    pub planning_selection: &'a mut Option<String>,
}

pub(super) fn paint(
    ui: &mut egui::Ui,
    lane: Lane<'_>,
) -> (BTreeSet<String>, BTreeSet<String>, BTreeSet<String>) {
    let Lane {
        surface,
        board,
        planning,
        tasks,
        items,
        column,
        selected_path,
        planning_selection,
    } = lane;
    let mut groups = collect_groups(board, planning, tasks, items);
    add_root_lane_groups(board, planning, &mut groups, items);
    linked::collect(surface, board, column, &mut groups);

    let mut consumed_work = BTreeSet::new();
    let mut consumed_tasks = BTreeSet::new();
    let mut consumed_items = BTreeSet::new();
    for (uid, group) in groups {
        let Some(root) = group.root else {
            continue;
        };
        if planning.iter().any(|work| work.uid == root.uid) {
            consumed_work.insert(root.uid.clone());
        }
        let count = group.planning.len()
            + group.tasks.len()
            + group.items.len()
            + group.pr_work.len()
            + group.approvals.len()
            + group.plan_reviews.len()
            + group.clarifications.len();
        let id = egui::Id::new("koolade_task_intent_collapsed").with(&uid);
        let mut collapsed = ui
            .ctx()
            .data_mut(|data| data.get_temp::<BTreeSet<String>>(id).unwrap_or_default());
        egui::Frame::NONE
            .fill(theme::PANEL_ALT.gamma_multiply(0.5))
            .stroke(egui::Stroke::new(1.0, theme::BORDER))
            .corner_radius(8)
            .inner_margin(8)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    theme::badge(ui, "TASK INTENT", theme::PANEL, theme::TEXT_DIM);
                    let is_collapsed = collapsed.contains(&uid);
                    if ui
                        .small_button(if is_collapsed { "Expand" } else { "Collapse" })
                        .clicked()
                    {
                        if is_collapsed {
                            collapsed.remove(&uid);
                        } else {
                            collapsed.insert(uid.clone());
                        }
                    }
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new(task_cards::card_summary(&root.title))
                                    .strong()
                                    .size(14.0),
                            )
                            .frame(false),
                        )
                        .clicked()
                    {
                        *planning_selection = Some(root.key.clone());
                    }
                    theme::badge(
                        ui,
                        &format!(
                            "{} {} in this column",
                            count,
                            if count == 1 { "card" } else { "cards" }
                        ),
                        theme::PANEL,
                        theme::TEXT_DIM,
                    );
                    theme::badge(
                        ui,
                        lifecycle(root, board).label(),
                        theme::PANEL_ALT,
                        theme::TEXT_DIM,
                    );
                });
                ui.add_space(2.0);
                if !collapsed.contains(&uid) {
                    for question in &group.clarifications {
                        cards::clarification(ui, question, column, planning_selection);
                    }
                    for action in &group.approvals {
                        cards::approval(ui, action, planning_selection);
                    }
                    for feature in &group.plan_reviews {
                        cards::plan_review(ui, feature, planning_selection);
                    }
                    for task in &group.pr_work {
                        cards::pr_task(ui, task, column, selected_path);
                    }
                    for work in &group.planning {
                        consumed_work.insert(work.uid.clone());
                        cards::work(ui, surface, work, board, column, planning_selection);
                    }
                    for task in &group.tasks {
                        consumed_tasks.insert(task.path.clone());
                        cards::task(ui, surface, board, task, column, selected_path);
                    }
                    for item in &group.items {
                        consumed_items.insert(item.id.clone());
                        cards::item(ui, surface, board, item, column, planning_selection);
                    }
                } else {
                    consumed_work.extend(group.planning.iter().map(|work| work.uid.clone()));
                    consumed_tasks.extend(group.tasks.iter().map(|task| task.path.clone()));
                    consumed_items.extend(group.items.iter().map(|item| item.id.clone()));
                }
            });
        ui.add_space(8.0);
        ui.ctx().data_mut(|data| data.insert_temp(id, collapsed));
    }
    (consumed_work, consumed_tasks, consumed_items)
}

#[cfg(test)]
#[path = "task_intents_tests.rs"]
mod tests;

fn lifecycle(
    root: &crate::core::planning_work::Work,
    board: &crate::ui::planning_board::ViewModel,
) -> crate::core::task_intent::TaskIntentLifecycle {
    board.task_intent_lifecycle(root)
}
