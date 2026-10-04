use super::*;
// ---- Consolidated disk effects: ONE claim, phases A–F --------------

/// Runs the full disk-effect journey under ONE claim:
/// A fresh home seeds (+ dim note names the live path);
/// B unmodified save is churn-free (mtime steady, no temp debris);
/// C a custom markdown document round-trips byte-exact;
/// D blank attempts are gated without disturbing the good bytes;
/// E a corrupt home warns verbatim, is untouched by the mere open, and
///   Restore default heals it (amber clears, green confirms);
/// F a read-only home maps the io error to the red line with the staged
///   buffer preserved and NOTHING written.
/// Contention with the other KOOLADE_HOME suites retrains as a quiet
/// re-attempt (see the layering notes above).
#[test]
fn persona_card_disk_effects_phases_a_through_f() {
    for attempt in 1..=JOURNEY_ATTEMPTS {
        let outcome = journey_phases_abcd_ef();
        match outcome {
            Ok(()) => return,
            Err(_) if attempt < JOURNEY_ATTEMPTS => {
                std::thread::sleep(Duration::from_millis(u64::from(attempt) * 7));
            }
            Err(other) => panic!("disk-effect journey lost its ground: {other:?}"),
        }
    }
    unreachable!()
}

fn journey_phases_abcd_ef() -> Result<(), Contested> {
    let mut claim = EnvClaim::begin("journey")?;

    // Phase A — FRESH home: opening is the first-run seed trigger; the
    // dim note names the live persona.md path; constant bytes on disk.
    let home_a = claim.new_phase("a_seed");
    claim.checkpoint()?;
    let load_a = persona::load_persona();
    assert!(load_a.seeded_now, "fresh home must seed on first load");
    let mut card = DlgPersona::from_load(&load_a);
    assert_eq!(card.document, SHIPPED_DEFAULT_PERSONA);
    let note = card
        .seeded_note
        .as_ref()
        .expect("a healthy first-run seed announces itself");
    let shown_path = persona::persona_path().display().to_string();
    assert!(
        note.contains(&shown_path),
        "note {note:?} must name {shown_path}"
    );
    assert_eq!(card.warning, None);
    assert_eq!(
        std::fs::read(persona::persona_path()).unwrap(),
        SHIPPED_DEFAULT_PERSONA.as_bytes(),
        "seeded file equals the shipped default exactly"
    );

    // Phase B — UNMODIFIED card: save() is Unchanged with NO write
    // churn (mtime unchanged) and the home lists exactly persona.md —
    // no stale temp.
    claim.checkpoint()?;
    let mtime_b = std::fs::metadata(persona::persona_path())
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(card.save(), Ok(PersonaSaveOutcome::Unchanged));
    assert_eq!(
        std::fs::metadata(persona::persona_path())
            .unwrap()
            .modified()
            .unwrap(),
        mtime_b,
        "an in-sync save must not rewrite the file"
    );
    let mut names: Vec<String> = std::fs::read_dir(&home_a)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec!["persona.md".to_owned()],
        "no temp debris after an in-sync save"
    );

    // Phase C — CUSTOM multi-line markdown (bullets, bold markers, an
    // em dash): save() is Written, bytes equal exactly, notes/warning
    // clear, and a second save confirms the baseline tracked the write.
    claim.checkpoint()?;
    let custom = "# Tuned Voice\n\n- **Bold beat** and `code`\n- Second beat \u{2014} an em dash\n\nProse paragraph.\n";
    card.document = custom.to_owned();
    assert_eq!(card.save(), Ok(PersonaSaveOutcome::Written));
    assert_eq!(
        std::fs::read(persona::persona_path()).unwrap(),
        custom.as_bytes(),
        "saved bytes must equal the buffer exactly"
    );
    assert_eq!(card.warning, None, "known-good bytes clear the warning");
    assert_eq!(
        card.save(),
        Ok(PersonaSaveOutcome::Unchanged),
        "the baseline must track the write"
    );

    // Phase D — WHITESPACE buffers: three blank attempts all refuse and
    // the good bytes survive untouched; the red line cites the guard.
    claim.checkpoint()?;
    for blank in ["", " \t\n", "   "] {
        card.document = blank.to_owned();
        assert!(card.save().is_err(), "blank {blank:?} must be refused");
    }
    assert_eq!(
        std::fs::read(persona::persona_path()).unwrap(),
        custom.as_bytes(),
        "blank attempts never reach the disk"
    );
    let (_, msg) = card
        .feedback
        .as_ref()
        .expect("blank refusals set the red line");
    assert!(msg.contains("must not be blank"), "{msg}");
    card.document = custom.to_owned(); // re-stage the good text

    // Phase E — CORRUPT home: the open warns verbatim WITHOUT healing;
    // Restore default heals, the amber line clears, green confirms.
    claim.new_phase("e_corrupt");
    claim.checkpoint()?;
    let corrupt: &[u8] = &[0xFF, 0xFE, 0xFD, 0xFC];
    std::fs::write(persona::persona_path(), corrupt).unwrap();
    let load_e = persona::load_persona();
    assert!(load_e.fell_back_to_default, "corrupt bytes fall back");
    let diag = load_e.diagnostic.clone().expect("fallback diagnosed");
    let mut card = DlgPersona::from_load(&load_e);
    assert_eq!(card.document, SHIPPED_DEFAULT_PERSONA);
    assert_eq!(
        card.warning.as_deref(),
        Some(diag.as_str()),
        "the store diagnostic is the amber line, verbatim"
    );
    assert_eq!(
        std::fs::read(persona::persona_path()).unwrap(),
        corrupt,
        "the MERE OPEN must not heal the corrupt bytes"
    );
    card.restore_default()
        .expect("restore heals a corrupt file");
    assert_eq!(
        std::fs::read(persona::persona_path()).unwrap(),
        SHIPPED_DEFAULT_PERSONA.as_bytes(),
        "restored bytes equal the shipped default exactly"
    );
    assert_eq!(
        card.warning, None,
        "successful restore clears the amber line"
    );
    let (ok, msg) = card.feedback.as_ref().expect("restore confirmation line");
    assert!(ok, "{msg}");
    assert!(msg.contains("Restored"), "{msg}");

    // Phase F — READ-ONLY home (unix perms): the seed write fails, the
    // load falls back with a diagnostic, and a save is refused with the
    // MAPPED io error — red line citing the OS message, staged buffer
    // preserved, NOTHING written (not even a temp file).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let home_f = claim.new_phase("f_ro");
        claim.checkpoint()?;
        assert!(
            persona::load_persona().seeded_now,
            "setup: the still-writable ro-home seeds first"
        );
        std::fs::remove_file(persona::persona_path()).unwrap();
        std::fs::set_permissions(&home_f, std::fs::Permissions::from_mode(0o500)).unwrap();
        let load_f = persona::load_persona();
        assert!(
            load_f.fell_back_to_default,
            "unseedable absence reads as fallback"
        );
        let mut card = DlgPersona::from_load(&load_f);
        assert_eq!(card.warning, load_f.diagnostic, "fallback diagnostic shown");
        card.document = "# My voice\n".to_owned();
        let err = card
            .save()
            .expect_err("a read-only home must refuse the write");
        match err {
            AppError::Io { detail, .. } => assert!(
                detail.contains("Permission denied") || detail.contains("os error 13"),
                "the io detail must carry the OS message: {detail}"
            ),
            other => panic!("expected the mapped Io error, got: {other:?}"),
        }
        assert_eq!(
            card.document, "# My voice\n",
            "the staged buffer is preserved"
        );
        let (ok, msg) = card.feedback.as_ref().expect("red line set");
        assert!(!ok && msg.contains("save persona"), "{msg}");
        assert!(
            !persona::persona_path().exists(),
            "nothing may have been written into the ro home"
        );
        assert!(
            std::fs::read_dir(&home_f).unwrap().next().is_none(),
            "even a leftover temp file must not linger in the ro home"
        );
        std::fs::set_permissions(&home_f, std::fs::Permissions::from_mode(0o700)).unwrap();
        claim.checkpoint()?;
    }

    Ok(())
}
