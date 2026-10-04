use super::*;

// Planner persona card (operator-level persona store, story 002 front end)
// ---------------------------------------------------------------------------

/// Mandated one-line subordination notice (boundary ruling CLR-022 / DE-3):
/// the persona tunes the planner VOICE AND PRINCIPLES ONLY — the application
/// envelope, routing, and safety rails stay in force. The copy is word-pinned
/// by the test module below against later rephrases.
pub const PERSONA_SUBORDINATION_NOTICE: &str = "Tunes the planner voice and principles only \u{2014} the application envelope, routing, and safety rails stay in force.";

/// Outcome of [`DlgPersona::save`]: the buffer was already in sync (zero IO
/// performed) or the store's bytes were rewritten to the buffer's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersonaSaveOutcome {
    /// `document == base`: no write happened — no file touch at all.
    Unchanged,
    /// `persona.md` was atomically rewritten to the buffer's bytes.
    Written,
}

/// Planner persona card hosted in the Workspace settings modal. Mirrors the
/// [`DlgMcp`] discipline: raw free-text (standard markdown) editing, explicit
/// save from the footer, a sticky amber warning, and an ok/failure feedback
/// line — with every BUSINESS EFFECT confined to the effect methods below.
/// Effects run on SAVE / RESTORE only; reads never heal or rewrite. The card
/// itself carries no egui state: the hosting layout keeps it in session-
/// volatile temp data (bind-on-open doctrine).
#[derive(Clone, Default)]
pub struct DlgPersona {
    /// The editor buffer; bound to the file's live bytes on open.
    pub document: String,
    /// Baseline known-good bytes for unchanged-detection and the no-churn
    /// short-circuit. Advanced ONLY after a successful write.
    pub(super) base: String,
    /// First-run seed announcement (dim info line), naming the seeded file.
    pub(super) seeded_note: Option<String>,
    /// Sticky amber line: the store's fallback diagnostic, verbatim. Cleared
    /// only when a successful write establishes known-good bytes.
    pub(super) warning: Option<String>,
    /// Feedback line colored green/red by the ok flag.
    pub(super) feedback: Option<(bool, String)>,
}

impl DlgPersona {
    /// Bind the card to a store load. Pure with respect to the filesystem:
    /// it clones strings and renders the persona path for the note — zero
    /// disk IO, ever (the SEEDING write happens in the layout's first-ever
    /// `load_persona` call, not here).
    pub fn from_load(load: &persona::PersonaLoad) -> Self {
        let path = persona::persona_path().display().to_string();
        Self {
            document: load.document.clone(),
            base: load.document.clone(),
            seeded_note: load
                .seeded_now
                .then(|| format!("First run: seeded {path} with the shipped four-beat default.")),
            warning: load
                .fell_back_to_default
                .then(|| load.diagnostic.clone())
                .flatten(),
            feedback: None,
        }
    }

    /// Explicit save (the ONLY effect path besides restore).
    ///
    /// An unedited buffer (`document == base`) short-circuits to
    /// [`PersonaSaveOutcome::Unchanged`] before ANY IO — no write churn, no
    /// mtime bump, no temp debris. Otherwise the store's pre-disk blank
    /// guard surfaces as the friendly red line and every other failure kind
    /// maps to an io-detail error carrying the OS message. On success `base`
    /// advances and the notes/warning clear (bytes now known-good);
    /// on failure the buffer stands as-is and the RED FEEDBACK LINE IS
    /// ALREADY SET, so the modal stays open showing why.
    pub fn save(&mut self) -> Result<PersonaSaveOutcome, AppError> {
        if self.document == self.base {
            return Ok(PersonaSaveOutcome::Unchanged);
        }
        match persona::save_persona(&self.document) {
            Ok(()) => {
                self.base = self.document.clone();
                self.seeded_note = None;
                self.warning = None;
                self.feedback = Some((true, "Persona saved.".to_owned()));
                Ok(PersonaSaveOutcome::Written)
            }
            Err(err) => {
                let app_err = Self::map_save_error(&err);
                self.feedback = Some((false, Self::error_text(&app_err)));
                Err(app_err)
            }
        }
    }

    /// Prominent restore: stage the shipped constant, then persist it.
    ///
    /// Success clears the amber line and confirms green ("Restored the
    /// shipped default persona."). A failure KEEPS the staged buffer in the
    /// editor and sets the red line explaining the write failure, so the
    /// operator sees the intent survived. This write is the ONLY sanctioned
    /// healer of a corrupt/deleted file (story 001: reads never heal).
    pub fn restore_default(&mut self) -> Result<(), AppError> {
        self.stage_default();
        match persona::save_persona(&self.document) {
            Ok(()) => {
                self.warning = None;
                self.feedback = Some((true, "Restored the shipped default persona.".to_owned()));
                Ok(())
            }
            Err(err) => {
                let app_err = Self::map_save_error(&err);
                let text = format!(
                    "{} \u{2014} the staged default stays in the editor; make the home writable and press Restore default again.",
                    Self::error_text(&app_err)
                );
                self.feedback = Some((false, text));
                Err(app_err)
            }
        }
    }

    /// Pure staging half of a restore: point the buffer AND the baseline at
    /// the shipped constant and drop the note/feedback. Private: nothing but
    /// [`Self::restore_default`] may stage without persisting. The amber
    /// `warning` is deliberately KEPT here — it clears only when the write
    /// lands.
    pub(super) fn stage_default(&mut self) {
        self.document = persona::SHIPPED_DEFAULT_PERSONA.to_string();
        self.base = persona::SHIPPED_DEFAULT_PERSONA.to_string();
        self.seeded_note = None;
        self.feedback = None;
    }

    /// Store io → AppError mapping: the pre-disk blank guard becomes the
    /// friendly string-detail line; every other kind carries the OS message
    /// as an io-detail.
    fn map_save_error(err: &std::io::Error) -> AppError {
        if err.kind() == std::io::ErrorKind::InvalidData {
            AppError::Other(
                "Cannot save a blank persona \u{2014} the document must not be blank".to_owned(),
            )
        } else {
            AppError::Io {
                op: "save persona".to_owned(),
                detail: err.to_string(),
            }
        }
    }

    /// One-line rendering of a mapped error for the red feedback line (keeps
    /// the Other payload single rather than doubled through `Display`).
    fn error_text(err: &AppError) -> String {
        match err {
            AppError::Other(message) => message.clone(),
            AppError::Io { op, detail } => format!("{op}: {detail}"),
            other => other.headline(),
        }
    }
}

/// Paint the Planner persona card; returns (save_pressed, restore_pressed).
/// Strictly inert — it reads card state and performs NO file IO; the effect
/// methods own every mutation. Sizing mirrors [`paint_mcp_card`] (heading,
/// dim one-liner, monospace multiline editor, 10.5pt char meter, action
/// row). No Close control: the modal's X owns dismissal, and the shared
/// [`footers`] helper stays byte-identical for the four pre-existing dialogs.
pub fn paint_persona_card(ui: &mut egui::Ui, card: &mut DlgPersona) -> (bool, bool) {
    ui.label(
        RichText::new("Planner persona")
            .size(13.0)
            .strong()
            .color(theme::TEXT),
    );
    ui.add_space(2.0);
    // The pinned subordination notice (CLR-022 / DE-3 boundary ruling).
    ui.label(
        RichText::new(PERSONA_SUBORDINATION_NOTICE)
            .weak()
            .size(11.0),
    );
    // First-run seed announcement (dim info line), then the store's
    // fallback diagnostic, VERBATIM, as the sticky amber warning.
    if let Some(note) = &card.seeded_note {
        ui.add_space(4.0);
        ui.label(RichText::new(note).size(11.5).weak().color(theme::TEXT_DIM));
    }
    if let Some(warning) = &card.warning {
        ui.add_space(4.0);
        ui.label(
            RichText::new(warning)
                .size(11.5)
                .weak()
                .color(theme::WARNING),
        );
    }
    ui.add_space(5.0);
    // Raw free-text standard-markdown editor: nine desired rows, scrolls
    // vertically; the height budget tracks the per-line pacing of the other
    // in-file editors (~21 px/row), degrading to extra scroll in narrow
    // windows — never horizontal clipping.
    ui.add_sized(
        egui::vec2(ui.available_width(), 190.0),
        TextEdit::multiline(&mut card.document)
            .font(egui::FontId::monospace(12.0))
            .desired_width(f32::INFINITY)
            .desired_rows(9),
    );
    ui.label(
        RichText::new(format!("{} chars", card.document.chars().count()))
            .size(10.5)
            .weak()
            .color(theme::TEXT_DIM),
    );
    if let Some((ok, msg)) = &card.feedback {
        ui.add_space(8.0);
        ui.label(RichText::new(msg).size(11.5).color(if *ok {
            theme::SUCCESS
        } else {
            theme::DANGER
        }));
    }
    let mut save_pressed = false;
    let mut restore_pressed = false;
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        let restore = ui
            .add(
                egui::Button::new(RichText::new("Restore default").strong().color(theme::BG))
                    .fill(theme::PANEL_ALT)
                    .corner_radius(6.0),
            )
            .on_hover_text("Rewrite the editor with the shipped four-beat default and save it");
        ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
            let save = ui
                .add(
                    egui::Button::new(RichText::new("Save").strong().color(theme::BG))
                        .fill(theme::ACCENT_SOFT)
                        .corner_radius(6.0),
                )
                .on_hover_text("Persist the editor text to persona.md in the operator home");
            if save.clicked() {
                save_pressed = true;
            }
        });
        if restore.clicked() {
            restore_pressed = true;
        }
    });
    (save_pressed, restore_pressed)
}
