use super::*;
// ---- Pure mapping/effect pins (NO env, NO disk) --------------------

/// Plain load: document bytes copied EXACTLY, base mirrored, every note
/// and feedback starts clean.
#[test]
fn from_load_plain_load_copies_bytes_mirrors_base_starts_clean() {
    let text = "Stored text T\nline two\n";
    let card = DlgPersona::from_load(&fixture(text, false, false, None));
    assert_eq!(card.document, text);
    assert_eq!(card.base, text);
    assert_eq!(card.seeded_note, None);
    assert_eq!(card.warning, None);
    assert_eq!(card.feedback, None);
}

/// Seeded-now load: the first-run seed announces itself with the dim
/// INFO line NAMEING THE SEEDED FILE, and an incidental diagnostic is
/// ignored — seeding is a happy path, not a fallback (bytes exact,
/// warning stays clean).
#[test]
fn from_load_seeded_now_announces_named_file_without_warning() {
    let card = DlgPersona::from_load(&fixture(
        SHIPPED_DEFAULT_PERSONA,
        true,
        false,
        Some("absent; shipped default seeded".to_owned()),
    ));
    assert_eq!(card.document, SHIPPED_DEFAULT_PERSONA);
    assert_eq!(card.base, SHIPPED_DEFAULT_PERSONA);
    let note = card
        .seeded_note
        .as_ref()
        .expect("a first-run seed must announce itself");
    assert!(
        note.contains("persona.md"),
        "the info line names the seeded file: {note:?}"
    );
    assert_eq!(
        card.warning, None,
        "seeding is a happy path, not a fallback"
    );
    assert_eq!(card.feedback, None);
}

/// Fallen-back load: the STORE DIAGNOSTIC becomes the sticky warning,
/// verbatim; no seed note, no feedback.
#[test]
fn from_load_fallen_back_load_lifts_diagnostic_verbatim_into_warning() {
    let diagnostic = "persona file /x/persona.md is not valid UTF-8; serving the shipped default";
    let card = DlgPersona::from_load(&fixture(
        SHIPPED_DEFAULT_PERSONA,
        false,
        true,
        Some(diagnostic.to_owned()),
    ));
    assert_eq!(card.document, SHIPPED_DEFAULT_PERSONA);
    assert_eq!(card.base, SHIPPED_DEFAULT_PERSONA);
    assert_eq!(
        card.warning.as_deref(),
        Some(diagnostic),
        "the store's diagnosis travels verbatim as the amber line"
    );
    assert_eq!(card.seeded_note, None);
    assert_eq!(card.feedback, None);
}

/// Pure restore staging: buffer AND baseline jump to the shipped
/// constant; notes and feedback clear; the constant is the FIXED
/// spelling (the brief's 'Inqsitive' typo stays normalized).
#[test]
fn stage_default_stages_constant_and_clears_notes_and_feedback() {
    let mut card = DlgPersona::from_load(&fixture(
        "# Drifted voice\n\nedited\n",
        true,
        false,
        Some("seed".to_owned()),
    ));
    card.feedback = Some((false, "prior failure".to_owned()));
    card.stage_default();
    assert_eq!(card.document, SHIPPED_DEFAULT_PERSONA);
    assert_eq!(card.base, SHIPPED_DEFAULT_PERSONA);
    assert_eq!(card.seeded_note, None);
    assert_eq!(card.feedback, None);
    assert!(
        SHIPPED_DEFAULT_PERSONA.contains("Inquisitive"),
        "shipped default keeps the normalized spelling"
    );
    assert!(
        !SHIPPED_DEFAULT_PERSONA.contains("Inqsitive"),
        "the brief's misspelling must stay normalized away"
    );
}

/// No-op save short-circuits with ZERO IO, and blank buffers trip the
/// blank guard BEFORE any disk touch (the guard returns ahead of the
/// path lookup, so these legs need no home at all). The buffer stands
/// as-is after each refusal and the red line quotes the guard.
#[test]
fn unedited_save_short_circuits_and_blank_buffers_gate_before_disk() {
    let mut card = DlgPersona::from_load(&fixture("Stored text T\n", false, false, None));
    assert_eq!(card.save(), Ok(PersonaSaveOutcome::Unchanged));
    assert_eq!(card.document, "Stored text T\n");
    assert_eq!(card.feedback, None);
    for (label, blank) in [
        ("empty string", ""),
        ("space plus tab plus newline", " \t\n"),
        ("spaces only", "   "),
    ] {
        card.document = blank.to_owned();
        let err = card.save().unwrap_err();
        assert!(
            matches!(&err, AppError::Other(msg) if msg.contains("must not be blank")),
            "{label} must trip the blank guard with a friendly string error: {err:?}"
        );
        assert_eq!(card.document, blank, "{label}: the buffer stands as-is");
        let (ok, msg) = card
            .feedback
            .as_ref()
            .expect("{label}: the red feedback line is set");
        assert!(!ok && msg.contains("must not be blank"), "{label}: {msg}");
    }
}

/// Subordination copy pin: the mandated ONE-LINE notice keeps its load-
/// bearing words against later rephrases.
#[test]
fn subordination_notice_pins_one_line_with_voice_principles_and_rails() {
    let notice = PERSONA_SUBORDINATION_NOTICE;
    assert_eq!(notice.lines().count(), 1, "the notice is ONE line");
    for word in ["voice", "principles", "rails"] {
        assert!(notice.contains(word), "notice dropped '{word}': {notice}");
    }
}
