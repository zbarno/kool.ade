use super::*;

/// Placeholder for the first-run editor — ITSELF valid JSON so pasting the
/// hint straight into Save passes the well-formedness probe.
pub const MCP_EXAMPLE_HINT: &str = "{\n  \"mcpServers\": {\n    \"example\": { \"command\": \"your-server\", \"args\": [] }\n  }\n}";

/// F-18 / D-16: the in-app editor for `.koolade-packet/config/mcp.json`. Raw
/// free-text, mono-spaced; write/remove + checkpoint is owned by
/// `crate::artifacts::mcp_io`, which keeps this struct a dumb carrier.
pub struct DlgMcp {
    /// The editor buffer; bound to the file's live bytes on open.
    pub text: String,
    /// True when the field opened from NO existing file — the painter then
    /// shows the exemplar hint as placeholder.
    pub hint_active: bool,
    pub feedback: Option<(bool, String)>,
    /// Sticky, non-blocking warning (orange) — set on a malformed-yet-saved
    /// buffer or an unreadable pre-existing file; survives until Close.
    pub warning: Option<String>,
    /// Keep-open driver owned by `perform_mcp`: after a malformed save the
    /// card MUST stay up showing the warning; Unchanged/Write/Clear closes.
    pub keep_open: bool,
}

impl DlgMcp {
    /// Seed the card from the LIVE file bytes (first-run: absent → hint).
    pub fn from_project(proj: &Project) -> Self {
        let st = crate::artifacts::mcp_io::load_state(&proj.state.repo_root);
        let (text, hint_active, warning) = Self::dialog_fields(&st);
        Self {
            text,
            hint_active,
            feedback: None,
            warning,
            keep_open: true,
        }
    }

    /// Factor the load → field mapping so the pins exercise the REAL
    /// branching (absent / present-ok / present-unreadable) without egui.
    /// Unreadable ⇒ the field starts empty WITH an explicit OVERWRITE
    /// warning: unseen content is never destroyed silently.
    pub(super) fn dialog_fields(
        st: &crate::artifacts::mcp_io::McpLoadState,
    ) -> (String, bool, Option<String>) {
        if !st.present {
            (String::new(), true, None)
        } else if let Some(content) = &st.content {
            (content.clone(), false, None)
        } else {
            let err = st
                .read_error
                .clone()
                .unwrap_or_else(|| "unknown read error".into());
            (
                String::new(),
                false,
                Some(format!(
                    "Existing .koolade-packet/config/mcp.json could not be read ({err}). The field starts empty — saving will OVERWRITE the file."
                )),
            )
        }
    }

    /// Thin shim onto the module that owns all disk/git effects. No
    /// `PlannerState` resync is owed: mcp.json is never cached in state —
    /// the next turn's `context_build` re-reads the file fresh.
    pub fn apply(
        &mut self,
        proj: &mut Project,
    ) -> Result<crate::artifacts::mcp_io::McpApplyReceipt, AppError> {
        crate::artifacts::mcp_io::apply_save(&proj.state.repo_root, &self.text)
    }
}

// ---------------------------------------------------------------------------
/// Paint the MCP card; returns (save_pressed, close_pressed). Height budget
/// (~360 px) fits the 640 px min window — no ScrollArea, unlike the grown
/// settings card.
pub fn paint_mcp_card(ui: &mut egui::Ui, dlg: &mut DlgMcp) -> (bool, bool) {
    ui.label(
        RichText::new("MCP server configuration")
            .size(13.0)
            .strong()
            .color(theme::TEXT),
    );
    ui.add_space(3.0);
    ui.label(
        RichText::new(
            "Stored as .koolade-packet/config/mcp.json. Kool.ad/e shows the planner server names only; commands and credentials stay hidden. The editor checks JSON syntax only; blank + Save removes the file.",
        ).weak().size(11.0),
    );
    ui.add_space(4.0);
    // Multiline pattern mirrors the import dialog; the exemplar hint is only
    // offered while the field opened with no file underneath (hint_active).
    let editor = TextEdit::multiline(&mut dlg.text)
        .font(egui::FontId::monospace(12.0))
        .desired_width(f32::INFINITY)
        .desired_rows(10);
    let editor = if dlg.hint_active {
        editor.hint_text(MCP_EXAMPLE_HINT)
    } else {
        editor
    };
    ui.add_sized(egui::vec2(ui.available_width(), 210.0), editor);
    // The verified feed cap: prompts clip past 4,096 chars (context_build).
    // DISK NEVER clips — this line only telegraphs the presentation cutoff.
    ui.label(
        RichText::new(format!(
            "{} chars — prompts clip past 4096",
            dlg.text.chars().count()
        ))
        .size(10.5)
        .weak()
        .color(theme::TEXT_DIM),
    );
    if let Some(warning) = &dlg.warning {
        ui.add_space(6.0);
        ui.label(
            RichText::new(warning)
                .size(11.5)
                .weak()
                .color(theme::WARNING),
        );
    }
    footers(ui, &dlg.feedback)
}
