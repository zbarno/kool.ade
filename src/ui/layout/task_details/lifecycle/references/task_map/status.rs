pub fn node_status(ui: &mut egui::Ui, status: &str) {
    ui.label(node_status_text(status));
}

pub(super) fn node_status_text(status: &str) -> String {
    let normalized = status.to_ascii_lowercase().replace('_', " ");
    if normalized.contains("pr ") {
        let marker = if normalized.contains("attention") || normalized.contains("closed") {
            "!"
        } else if normalized.contains("merged") {
            "✓"
        } else if normalized.contains("open") {
            "↗"
        } else {
            "◷"
        };
        return format!("{marker} {status}");
    }
    let (marker, label) = if normalized.contains("running")
        || normalized.contains("implementing")
        || normalized.contains("inprogress")
        || normalized.contains("in progress")
    {
        ("●", "Running")
    } else if normalized.contains("awaiting review") || normalized.contains("review") {
        ("◷", "Awaiting review")
    } else if normalized.contains("answer saved") {
        ("◷", "Answer saved")
    } else if normalized.contains("runtime qa") || normalized.contains("quality assurance") {
        ("◷", "Awaiting runtime QA")
    } else if normalized.contains("spec") && normalized.contains("sync") {
        ("◷", "Awaiting specification sync")
    } else if normalized.contains("user")
        || normalized.contains("decision")
        || normalized.contains("input")
    {
        ("✋", "Needs user input")
    } else if normalized.contains("blocked") || normalized.contains("dependency") {
        ("⚠", "Blocked")
    } else if normalized.contains("cancel") {
        ("×", "Cancelled")
    } else if normalized.contains("fail") {
        ("!", "Failed")
    } else if normalized.contains("attention") || normalized.contains("needsattention") {
        ("!", "Needs attention")
    } else if normalized.contains("interrupt") {
        ("↻", "Interrupted")
    } else if normalized.contains("todo") {
        ("○", "Not started")
    } else if normalized.contains("done")
        || normalized.contains("complete")
        || normalized.contains("merged")
        || normalized.contains("resolved")
    {
        ("✓", "Completed")
    } else if normalized.contains("not recorded") || normalized.contains("unavailable") {
        ("?", "Status not recorded")
    } else if normalized.contains("waiting") || normalized.contains("awaiting") {
        ("…", status)
    } else {
        ("·", status)
    };
    format!("{marker} {label}")
}

#[cfg(test)]
#[path = "status_tests.rs"]
mod tests;
