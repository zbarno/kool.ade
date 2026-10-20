use super::super::super::super::*;

pub(in crate::ui::layout::board::columns) fn approval(
    ui: &mut egui::Ui,
    action: &crate::ui::feature_approval::Action,
    selected: &mut Option<String>,
) {
    let title = action
        .specification
        .lines()
        .find(|line| line.starts_with('#'))
        .map(|line| line.trim_start_matches('#').trim())
        .unwrap_or(&action.id);
    let key = action.board_key();
    let open =
        crate::ui::layout::task_cards::board_card(ui, &key, None, false, true, false, |ui| {
            ui.horizontal(|ui| {
                theme::badge(ui, "Plan confirmation", theme::PANEL, theme::TEXT_DIM);
                super::super::attention::badge(ui, super::super::attention::Kind::WaitingOnUser);
            });
            ui.label(RichText::new(title).strong().color(theme::TEXT));
            if ui.button("Review specification").clicked() {
                *selected = Some(key.clone());
            }
        });
    if open {
        *selected = Some(key);
    }
}

pub(in crate::ui::layout::board::columns) fn plan_review(
    ui: &mut egui::Ui,
    feature: &crate::core::lifecycle::FeatureGoal,
    selected: &mut Option<String>,
) {
    let key = format!("feature-plan-review:{}", feature.feature_uid);
    let title = feature
        .draft
        .lines()
        .find(|line| line.starts_with('#'))
        .map(|line| line.trim_start_matches('#').trim())
        .filter(|title| !title.is_empty())
        .unwrap_or(&feature.original_intent);
    let opened =
        crate::ui::layout::task_cards::board_card(ui, &key, None, false, false, false, |ui| {
            plan_review_content(ui, title, &key, selected)
        });
    if opened {
        *selected = Some(key);
    }
}

fn plan_review_content(ui: &mut egui::Ui, title: &str, key: &str, selected: &mut Option<String>) {
    ui.horizontal(|ui| {
        theme::badge(ui, "Plan Review", theme::PANEL, theme::TEXT_DIM);
        theme::badge(ui, "Independent", theme::PANEL, theme::ACTION);
    });
    if ui.button(egui::RichText::new(title).strong()).clicked() {
        *selected = Some(key.to_owned());
    }
    ui.label(theme::helper_text(
        "Reviewing the user-confirmed plan before task generation.",
    ));
    if ui.button("View review progress").clicked() {
        *selected = Some(key.to_owned());
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn confirmed_plan_review_has_a_card_in_the_review_lane() {
        let feature = crate::core::lifecycle::FeatureGoal {
            schema_version: 1,
            feature_uid: "feature-uid".into(),
            original_intent: "Improve task review".into(),
            draft: "# Task review".into(),
            scope: Vec::new(),
            non_goals: Vec::new(),
            revision: 2,
            phase: crate::core::lifecycle::FeaturePhase::PlanReviewPending,
            approved_plan_sha256: Some("a".repeat(64)),
            plan_snapshot_uid: Some("snapshot-uid".into()),
            plan_snapshot_sha256: Some("b".repeat(64)),
            unresolved_questions: Vec::new(),
            decision_refs: Vec::new(),
            review_refs: Vec::new(),
            readiness_evidence_refs: Vec::new(),
            task_uids: Vec::new(),
        };
        let context = egui::Context::default();
        assert_eq!(
            feature.phase,
            crate::core::lifecycle::FeaturePhase::PlanReviewPending
        );
        let title = feature
            .draft
            .lines()
            .next()
            .unwrap()
            .trim_start_matches('#')
            .trim();
        let mut selected: Option<String> = None;
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(480.0, 320.0),
                )),
                ..Default::default()
            },
            |ui| {
                super::plan_review_content(
                    ui,
                    title,
                    "feature-plan-review:feature-uid",
                    &mut selected,
                )
            },
        );
        output.textures_delta.clear();
        fn collect_text(shape: &egui::Shape, output: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(text) => output.push(text.galley.text().to_owned()),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect_text(shape, output);
                    }
                }
                _ => {}
            }
        }
        let mut rendered = Vec::new();
        for shape in &output.shapes {
            collect_text(&shape.shape, &mut rendered);
        }
        assert!(
            rendered.iter().any(|text| text == "Plan Review"),
            "rendered={rendered:?}"
        );
        assert!(rendered.iter().any(|text| text == "Independent"));
        assert!(rendered.iter().any(|text| text == "View review progress"));
        assert!(rendered.iter().any(|text| text == "Task review"));
    }
}
