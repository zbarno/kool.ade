use super::support::{TmpHome, persona_bytes};
use crate::persistence::persona::{
    SHIPPED_DEFAULT_PERSONA, load_persona, persona_path, save_persona,
};
use std::{fs, io};

#[test]
fn first_run_seeds_shipped_default_deterministically() {
    let _home = TmpHome::new("seed");
    assert!(SHIPPED_DEFAULT_PERSONA.contains("Kool.ad/e Man.ager"));
    assert!(SHIPPED_DEFAULT_PERSONA.contains("Kool.ad/e Man for short"));
    // Constant normative beats: spelled correctly, none of the typo.
    for beat in ["Concise", "Protective", "Inquisitive", "Creative"] {
        assert!(
            SHIPPED_DEFAULT_PERSONA.contains(beat),
            "shipped default must contain the beat `{beat}`"
        );
    }
    assert!(
        !SHIPPED_DEFAULT_PERSONA.contains("Inqsitive"),
        "the brief's misspelling must be normalized away"
    );

    // Pristine home: first load seeds loudly.
    let first = load_persona();
    assert!(first.seeded_now, "fresh home must seed on first load");
    assert!(!first.fell_back_to_default);
    assert_eq!(first.document, SHIPPED_DEFAULT_PERSONA);
    let diag = first.diagnostic.expect("seed must be diagnosed");
    let p = persona_path().to_string_lossy().to_string();
    assert!(
        diag.contains("absent"),
        "seed diagnostic {diag:?} should name the absence"
    );
    assert!(
        diag.contains(&p),
        "seed diagnostic {diag:?} should name the path"
    );
    assert_eq!(
        persona_bytes(),
        SHIPPED_DEFAULT_PERSONA.as_bytes(),
        "seeded file must be byte-equal to the shipped default"
    );

    // Immediate second load (relaunch simulation): identical bytes, no reseed.
    let second = load_persona();
    assert!(!second.seeded_now, "second load must not reseed");
    assert!(!second.fell_back_to_default);
    assert!(
        second.diagnostic.is_none(),
        "healthy load carries no diagnostic"
    );
    assert_eq!(second.document, SHIPPED_DEFAULT_PERSONA);
    assert_eq!(
        persona_bytes(),
        SHIPPED_DEFAULT_PERSONA.as_bytes(),
        "reload must leave the seeded bytes untouched"
    );
}

#[test]
fn custom_multiline_save_round_trips_across_relunches_without_residue() {
    let home = TmpHome::new("roundtrip");
    let custom = "# Custom Voice\n\n- Top level\n  - Nested bullet\n- Bold **pair** here \u{2014} em dash, \u{201C}typed quotes\u{201D}\n\nClosing prose.\n";
    save_persona(custom).expect("saving a well-formed document must succeed");

    let a = load_persona();
    assert_eq!(
        a.document, custom,
        "first load must be byte-identical to the saved input"
    );
    assert!(!a.fell_back_to_default);
    assert!(a.diagnostic.is_none());

    // Simulate a relaunch: successive loads stay byte-identical.
    let b = load_persona();
    assert_eq!(
        b.document, custom,
        "second (relaunch) load must be byte-identical"
    );
    assert!(!b.fell_back_to_default);
    assert!(b.diagnostic.is_none());
    assert_eq!(persona_bytes(), custom.as_bytes());

    // Home root carries exactly the persona file — no temp residue.
    let mut entries: Vec<String> = fs::read_dir(&home.home)
        .expect("home root must list")
        .map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()))
        .collect::<Result<Vec<_>, _>>()
        .expect("entry names should be valid UTF-8");
    entries.sort();
    assert_eq!(
        entries,
        vec!["persona.md".to_string()],
        "expected only persona.md, no temp residues"
    );
}

#[test]
fn invalid_utf8_file_falls_back_with_evidence_preserved() {
    let home = TmpHome::new("badutf8");
    fs::create_dir_all(&home.home).unwrap();
    // Byte 0xFF is outside the valid UTF-8 lead-byte range, followed by junk.
    let corrupt: &[u8] = &[0xFF, 0xFD, b'j', b'u', b'n', b'k'];
    fs::write(persona_path(), corrupt).unwrap();
    let before = persona_bytes();

    let r = load_persona();
    assert_eq!(r.document, SHIPPED_DEFAULT_PERSONA);
    assert!(
        r.fell_back_to_default,
        "invalid UTF-8 must trigger the fallback"
    );
    assert!(!r.seeded_now, "fallback must not pretend to have seeded");
    let diag = r.diagnostic.expect("fallback must be diagnosed");
    assert!(
        diag.contains("UTF-8"),
        "diagnostic {diag:?} should mention UTF-8"
    );
    assert_eq!(
        persona_bytes(),
        before,
        "corrupt bytes must stay on disk: no healing, no mutation"
    );
}

#[test]
fn blank_file_falls_back_naming_blank_and_stays_untouched() {
    let home = TmpHome::new("blankfile");
    fs::create_dir_all(&home.home).unwrap();
    let blank = "   \n\t\n  ";
    fs::write(persona_path(), blank).unwrap();

    let r = load_persona();
    assert_eq!(r.document, SHIPPED_DEFAULT_PERSONA);
    assert!(
        r.fell_back_to_default,
        "blank content must trigger the fallback"
    );
    assert!(!r.seeded_now);
    let diag = r.diagnostic.expect("fallback must be diagnosed");
    assert!(
        diag.contains("blank"),
        "diagnostic {diag:?} should name blank"
    );
    assert_eq!(
        persona_bytes(),
        blank.as_bytes(),
        "the blank file must be left untouched"
    );
}

#[test]
fn deleted_file_is_reseeded_loudly() {
    let home = TmpHome::new("deleted");
    fs::create_dir_all(&home.home).unwrap();
    fs::write(persona_path(), "# Something the operator tuned\n").unwrap();
    fs::remove_file(persona_path()).unwrap();

    let r = load_persona();
    assert!(r.seeded_now, "deletion must retrigger the first-run seed");
    assert!(
        !r.fell_back_to_default,
        "a successful reseed is not a fallback"
    );
    assert_eq!(r.document, SHIPPED_DEFAULT_PERSONA);
    let diag = r
        .diagnostic
        .expect("absence must be diagnosed, never silent");
    let p = persona_path().to_string_lossy().to_string();
    assert!(
        diag.contains("absent"),
        "diagnostic {diag:?} should name the absence"
    );
    assert!(
        diag.contains(&p),
        "diagnostic {diag:?} should name the path"
    );
    assert_eq!(
        persona_bytes(),
        SHIPPED_DEFAULT_PERSONA.as_bytes(),
        "re-seeded file must be byte-equal to the shipped default"
    );
}

#[test]
fn blank_saves_are_rejected_before_any_disk_touch() {
    let _home = TmpHome::new("savegate");
    let sentinel = "# Sentinel\n\n- left untouched\n";
    save_persona(sentinel).expect("sentinel save must succeed");

    for (label, candidate) in [
        ("empty string", ""),
        ("tab plus newline", "\t\n"),
        ("spaces only", "   "),
    ] {
        let outcome = save_persona(candidate);
        assert!(outcome.is_err(), "blank save ({label}) must be rejected");
        let err = outcome.unwrap_err();
        assert_eq!(
            err.kind(),
            io::ErrorKind::InvalidData,
            "blank save ({label}) must be rejected with InvalidData"
        );
        assert_eq!(
            persona_bytes(),
            sentinel.as_bytes(),
            "blank save ({label}) must leave the stored bytes untouched"
        );
    }

    // Positive control: saving the sentinel again succeeds and reloads byte-equal.
    save_persona(sentinel).expect("positive-control save must succeed");
    let r = load_persona();
    assert_eq!(r.document, sentinel);
    assert!(!r.fell_back_to_default);
    assert!(r.diagnostic.is_none());
}

#[test]
fn large_document_round_trips_without_size_cap() {
    let _home = TmpHome::new("bigdoc");
    let para = "Paragraph of operator-provided voice, with some length. ".repeat(8);
    let huge = format!("# Big persona\n\n{}\n", para.repeat(250)); // hundreds of KiB
    assert!(huge.len() >= 100 * 1024);
    save_persona(&huge).expect("large document must be savable (no size cap)");
    let r = load_persona();
    assert_eq!(
        r.document, huge,
        "large document must round-trip byte-exactly"
    );
    assert!(!r.fell_back_to_default);
    assert!(r.diagnostic.is_none());
}

#[test]
fn directory_where_the_file_should_be_is_unreadable_not_fatal() {
    let _home = TmpHome::new("dirext");
    fs::create_dir_all(persona_path()).unwrap();

    let r = load_persona();
    assert_eq!(
        r.document, SHIPPED_DEFAULT_PERSONA,
        "must keep serving a voice"
    );
    assert!(
        r.fell_back_to_default,
        "a directory at the path is corrupt-by-construction"
    );
    assert!(!r.seeded_now, "must not attempt to seed over a directory");
    let diag = r.diagnostic.expect("fallback must be diagnosed");
    assert!(
        diag.contains("unreadable"),
        "diagnostic {diag:?} should label the read failure"
    );
    assert!(
        persona_path().is_dir(),
        "the foreign directory must be left unmodified"
    );
}

#[cfg(unix)]
#[test]
fn unwritable_home_at_first_touch_returns_diagnosed_default_without_panicking() {
    use std::os::unix::fs::PermissionsExt;
    let home = TmpHome::new("rohome");
    fs::create_dir_all(&home.home).unwrap();
    let seeded = load_persona();
    assert!(seeded.seeded_now, "setup: writable home seeds first");
    fs::remove_file(persona_path()).unwrap();
    fs::set_permissions(&home.home, fs::Permissions::from_mode(0o500)).unwrap();

    let r = load_persona(); // must not panic despite the failed seed write
    assert_eq!(
        r.document, SHIPPED_DEFAULT_PERSONA,
        "voice must still be served"
    );
    assert!(
        r.fell_back_to_default,
        "unseedable absence must read as fallback"
    );
    assert!(!r.seeded_now);
    let diag = r.diagnostic.expect("failed seed write must be diagnosed");
    let p = persona_path().to_string_lossy().to_string();
    assert!(
        diag.contains(&p),
        "diagnostic {diag:?} should name the path"
    );
    assert!(
        diag.contains("seeding failed"),
        "diagnostic {diag:?} should cite the write failure"
    );

    // Defensive reversal before the guard's Drop re-applies it.
    fs::set_permissions(&home.home, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(
        !persona_path().exists(),
        "nothing may have been written into the ro home"
    );
}
