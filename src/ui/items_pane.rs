//! Right pane: live open-items queue, regrouped for the signed-in user.
//! Uses the routing layer directly under the D-14 law: `eligible_items`
//! (General broadcast; direct name or group address; a lane this user
//! owns, sole or via group; or an unowned lane this seat inherits) =
//! “FOR YOU NOW”; everything else — including lanes claimed by other
//! holders, which are NEVER posed here — stays in “THE REST OF THE
//! QUEUE”. Ownership-gap items ride the synthetic section.

use egui::{Frame, RichText};

use crate::core::routing;
use crate::domain::item::OpenItem;
use crate::domain::stakeholder::Stakeholders;
use crate::domain::user::CurrentUser;
use crate::ui::theme;

pub struct Args<'a> {
    pub items: &'a [OpenItem],
    pub synthetic: &'a [OpenItem],
    pub user: &'a CurrentUser,
    /// Category→owner map the D-14 law judges partition eligibility against.
    pub stakes: &'a Stakeholders,
    pub next_question_id: Option<&'a str>,
}

pub fn paint(ui: &mut egui::Ui, args: &Args<'_>) {
    if args.items.is_empty() && args.synthetic.is_empty() {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("All clear").strong().color(theme::SUCCESS));
            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "Questions, ambiguities and\nassumptions land here as the plan evolves.",
                )
                .weak()
                .size(12.0),
            );
        });
        return;
    }
    egui::ScrollArea::vertical()
        .auto_shrink(egui::Vec2b::new(false, false))
        .show(ui, |ui| {
            let mine: Vec<&OpenItem> = routing::eligible_items(args.items, args.user, args.stakes);
            let recommended =
                routing::recommended_next(args.items, args.user, args.stakes).filter(|item| {
                    item.authority == crate::domain::Authority::Human
                        && item.priority == crate::domain::Priority::Blocking
                });
            let recommended_id = recommended.map(|r| r.id.clone());
            let mine_ids: Vec<String> = mine.iter().map(|i| i.id.clone()).collect();

            section(ui, "FOR YOU NOW", theme::ACCENT, |ui| {
                if mine.is_empty() {
                    dim(ui, "nothing is waiting on you right now");
                }
                for it in &mine {
                    card(ui, it, Some(it.id.clone()) == recommended_id);
                }
            });
            section(ui, "THE REST OF THE QUEUE", theme::TEXT_DIM, |ui| {
                let rest: Vec<&OpenItem> = args
                    .items
                    .iter()
                    .filter(|i| !mine_ids.contains(&i.id))
                    .collect();
                if rest.is_empty() {
                    dim(ui, "No other items waiting");
                }
                for it in &rest {
                    card(ui, it, false);
                }
            });
            if !args.synthetic.is_empty() {
                section(ui, "CATEGORY NEEDS AN OWNER", theme::WARNING, |ui| {
                    for it in args.synthetic {
                        card(ui, it, false);
                    }
                });
            }
        });
}

fn dim(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).weak().size(11.5).italics());
    ui.add_space(4.0);
}

fn section<F>(ui: &mut egui::Ui, title: &str, tint: egui::Color32, body: F)
where
    F: FnOnce(&mut egui::Ui),
{
    ui.add_space(12.0);
    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new(title)
                .strong()
                .size(11.0)
                .extra_letter_spacing(0.2)
                .color(tint),
        );
    });
    ui.add_space(5.0);
    body(ui);
}

fn card(ui: &mut egui::Ui, item: &OpenItem, asking_now: bool) {
    let border = if asking_now {
        theme::ACCENT
    } else {
        theme::BORDER
    };
    let swatch = match item.priority {
        crate::domain::item::Priority::Blocking => theme::DANGER,
        crate::domain::item::Priority::High => theme::WARNING,
        crate::domain::item::Priority::Normal => theme::ACCENT,
    };
    Frame::NONE
        .fill(theme::PANEL)
        .corner_radius(12.0)
        .stroke(egui::Stroke::new(
            if asking_now { 1.5 } else { 1.0 },
            border,
        ))
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                let painter = ui.painter();
                painter.circle_filled(ui.cursor().min + egui::vec2(4.0, 5.0), 4.0, swatch);
                ui.add_space(9.0);
                ui.label(
                    RichText::new(&item.id)
                        .monospace()
                        .size(11.5)
                        .strong()
                        .color(theme::TEXT_DIM),
                );
                let (kb, kf) = item.kind.badge_colors();
                theme::badge(ui, item.kind.label(), kb, kf);
                let (pb, pf) = item.priority.badge_colors();
                theme::badge(ui, &priority_label(item), pb, pf);
                if asking_now {
                    theme::badge(ui, "ASKING NOW", theme::ACCENT_SOFT, theme::ACCENT);
                }
            });
            ui.add_space(5.0);
            ui.label(RichText::new(&item.question).color(theme::TEXT));
            ui.add_space(4.0);
            if !item.reason.trim().is_empty() {
                ui.label(
                    RichText::new(format!("↳ {}", item.reason.trim()))
                        .weak()
                        .size(11.0)
                        .italics(),
                );
                ui.add_space(4.0);
            }
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                ui.label(
                    RichText::new(category_line(&item.category))
                        .weak()
                        .size(10.5),
                );
                ui.label(
                    RichText::new(owner_line(item.assigned_to.as_deref().unwrap_or("-")))
                        .weak()
                        .size(10.5),
                );
            });
        });
    ui.add_space(7.0);
}

fn priority_label(item: &OpenItem) -> String {
    item.priority.to_string()
}

fn category_line(cat: &str) -> String {
    format!("▣ {cat}")
}

fn owner_line(owner: &str) -> String {
    format!("◉ {owner}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::item::{ItemKind, Priority};
    use crate::domain::stakeholder::CategoryOwners;

    fn mk(id: &str, kind: ItemKind, cat: &str, owner: &str, pri: Priority) -> OpenItem {
        OpenItem::new(
            id.into(),
            pri,
            kind,
            cat.into(),
            Some(owner.into()),
            format!("{id}?"),
            String::new(),
        )
    }

    #[test]
    fn arg_shapes_compile_against_real_domain() {
        let u = CurrentUser::new("Sam", vec!["Data".to_string()]);
        let items = vec![mk(
            "CLR-001",
            ItemKind::Question,
            "General",
            "All",
            Priority::Normal,
        )];
        let stakes = Stakeholders::default();
        let args = Args {
            items: &items,
            synthetic: &[],
            user: &u,
            stakes: &stakes,
            next_question_id: None,
        };
        let mine = routing::eligible_items(args.items, args.user, args.stakes);
        assert_eq!(mine.len(), 1);
        assert_eq!(
            routing::recommended_next(args.items, args.user, args.stakes).map(|r| r.id.clone()),
            Some("CLR-001".to_string())
        );
    }

    /// The FOR-YOU-NOW split tracks the D-14 evaluator: an unowned lane
    /// seat-inherits into the user's partition, while a lane sole-owned by
    /// somebody else stays in the remainder of the queue — visible, but
    /// never posed (no ASKING-NOW ring).
    #[test]
    fn partition_tracks_the_routing_law() {
        let u = CurrentUser::new("Zach", Vec::new());
        let stakes = Stakeholders::new(vec![
            CategoryOwners::new("Security", vec!["Morgan".into()]),
            CategoryOwners::new("InfoSec", Vec::new()),
        ]);
        let items = vec![
            mk(
                "CLR-001",
                ItemKind::Question,
                "InfoSec",
                "All",
                Priority::High,
            ),
            mk(
                "CLR-002",
                ItemKind::Question,
                "Security",
                "Morgan",
                Priority::Blocking,
            ),
        ];
        let mine = routing::eligible_items(&items, &u, &stakes);
        assert_eq!(
            mine.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
            vec!["CLR-001"],
            "the unowned InfoSec lane seat-inherits; the Morgan-sole Security lane must not"
        );
        let rec = routing::recommended_next(&items, &u, &stakes);
        assert_eq!(
            rec.map(|r| r.id.as_str()),
            Some("CLR-001"),
            "ASKING-NOW ring must crown the seat-inherited question"
        );
    }
}
