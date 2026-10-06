use super::super::*;

pub(super) fn paint(ui: &mut egui::Ui, view: &crate::ui::task_detail::ViewModel) {
    let Some(metrics) = &view.implementation_metrics else {
        return;
    };
    ui.collapsing("Implementation metrics", |ui| {
        paint_report(ui, metrics);
        if let Some(feature) = &view.feature_metrics {
            ui.add_space(8.0);
            ui.separator();
            ui.strong("Feature roll-up");
            paint_report(ui, feature);
        }
    });
}

fn paint_report(ui: &mut egui::Ui, report: &crate::persistence::telemetry::ImplementationMetrics) {
    row(
        ui,
        "Elapsed",
        report
            .elapsed_millis
            .map(duration)
            .as_deref()
            .unwrap_or("Not recorded"),
    );
    row(
        ui,
        "Active implementation time",
        &duration(report.active_implementation_millis),
    );
    row(
        ui,
        "AI execution time",
        &duration(report.ai_execution_millis),
    );
    row(
        ui,
        "Model calls",
        &report
            .model_calls
            .map(|value| value.to_string())
            .unwrap_or_else(|| "Not reported".into()),
    );
    ui.add_space(3.0);
    ui.strong("Tokens");
    row(ui, "Input", &optional(report.input_tokens));
    row(ui, "Cache read", &optional(report.cache_read_tokens));
    row(ui, "Cache write", &optional(report.cache_write_tokens));
    row(ui, "Output", &optional(report.output_tokens));
    row(ui, "Reasoning", &optional(report.reasoning_tokens));
    row(ui, "Total", &optional(report.total_tokens));
    row(
        ui,
        "Estimated API cost",
        &cost(report.estimated_cost_usd_micros),
    );
    if !report.by_harness_model.is_empty() {
        ui.add_space(3.0);
        ui.strong("By harness, provider, API, and model");
        for breakdown in &report.by_harness_model {
            ui.label(format!(
                "{} · {} · {} calls · {} tokens · {}",
                breakdown.label,
                duration(breakdown.duration_millis),
                optional(breakdown.model_calls),
                optional(breakdown.total_tokens),
                cost(breakdown.estimated_cost_usd_micros)
            ));
        }
    }
    if !report.by_phase.is_empty() {
        ui.add_space(3.0);
        ui.strong("By phase");
        for breakdown in &report.by_phase {
            ui.label(format!(
                "{} · {} · {} calls · {}",
                breakdown.label,
                duration(breakdown.duration_millis),
                optional(breakdown.model_calls),
                cost(breakdown.estimated_cost_usd_micros)
            ));
        }
    }
}

fn row(ui: &mut egui::Ui, name: &str, value: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(name).weak());
        ui.label(value);
    });
}

fn optional(value: Option<u64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "Not reported".into())
}

fn duration(millis: u64) -> String {
    let seconds = millis / 1000;
    let minutes = seconds / 60;
    let hours = minutes / 60;
    if hours > 0 {
        format!("{hours}h {}m {}s", minutes % 60, seconds % 60)
    } else if minutes > 0 {
        format!("{minutes}m {}s", seconds % 60)
    } else {
        format!("{seconds}s")
    }
}

fn cost(micros: Option<u64>) -> String {
    micros
        .map(|micros| format!("${}.{:06}", micros / 1_000_000, micros % 1_000_000))
        .unwrap_or_else(|| "Not reported".into())
}
