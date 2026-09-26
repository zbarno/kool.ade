//! Operator-level persona store: `persona.md` DIRECTLY under the state root.
//!
//! Ruling (editable-operator-persona feature, operator level): the planner's
//! voice is one markdown document owned by the operator, stored in the
//! `~/.packet` home (honoring `$PACKET_HOME`). [`persona_path`] derives from
//! the state root alone — never under `projects/<slug>/` — so the file spans
//! every connected project and can never sit inside a repository clone
//! (outside every git working tree by construction).
//!
//! Content is schema-less markdown: the operator owns it outright, saved
//! verbatim with no BOM/newline/size treatment (mirroring D-16's
//! save-any-value editor discipline). On first encounter the store seeds
//! [`SHIPPED_DEFAULT_PERSONA`] atomically. A missing, deleted, blank,
//! unreadable, or non-UTF-8 file serves the shipped default in memory plus a
//! diagnostic string in the return value — never a blank, never a panic,
//! never a propagated io error, and corrupt bytes are left on disk as
//! evidence (no healing rewrites).
//!
//! Idioms matched to [`super::chat_store`]: diagnostics travel ONLY in the
//! returned `Option<String>` (this store logs nothing; story 002 displays
//! them), and saves are atomic (`crate::artifacts::atomic_write`) so the file
//! is always wholly the old or wholly the new document; at most one stale
//! temp file may linger and the next successful rename displaces it.
//!
//! Concurrency posture: usage assumes one process and one active turn. A
//! two-instance first-touch seed race renames the identical constant bytes,
//! so last-writer-wins is a content no-op and determinism is preserved.

use std::fs;
use std::io;
use std::path::PathBuf;

/// The shipped four-beat default persona — normative bytes, exactly eight
/// lines, each terminated by a single LF: title, blank, intro, blank, then
/// the four bullets. The operator's four briefed beats with "Inqsitive"
/// normalized to "Inquisitive" (orthography fix only). Single source of
/// truth for seeding, story 002's Restore-default, and every
/// fallback-equality assertion.
pub const SHIPPED_DEFAULT_PERSONA: &str = concat!(
    "# Planner Persona (shipped default)",
    "\n",
    "",
    "\n",
    "Four standing beats:",
    "\n",
    "",
    "\n",
    "- Concise",
    "\n",
    "- Protective of the User, then the System, then the Project",
    "\n",
    "- Inquisitive",
    "\n",
    "- Creative",
    "\n",
);

/// `persona.md` joined directly onto the `$PACKET_HOME`-aware state root.
/// Deliberately NOT under `projects/<slug>/`: the persona is operator-level
/// and spans projects.
pub fn persona_path() -> PathBuf {
    crate::persistence::state_root().join("persona.md")
}

/// Outcome of [`load_persona`]: the document plus how it arrived.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonaLoad {
    /// Markdown to use: the verbatim stored file text (no trimming, no
    /// newline normalization) or the shipped default.
    pub document: String,
    /// True iff this call seeded the previously absent file with the
    /// shipped default.
    pub seeded_now: bool,
    /// True iff a corrupted/unreadable/blank file (or an absent file whose
    /// seed write failed) forced service of the shipped default.
    pub fell_back_to_default: bool,
    /// Diagnosis of the seed or fallback, embedding the persona path;
    /// `None` on a healthy load. Travelled to callers/display only — this
    /// store logs nothing of its own.
    pub diagnostic: Option<String>,
}

/// Load the operator persona.
///
/// Never panics and never propagates io errors:
/// * readable, valid UTF-8, non-blank → exact file text, no flags, no
///   diagnostic;
/// * absent → the shipped default is seeded atomically, `seeded_now` true,
///   plus an absence diagnostic;
/// * absent but the seed write itself fails (e.g. unwritable home) → the
///   in-memory shipped default, `fell_back_to_default` true, plus a
///   diagnostic citing the write failure;
/// * read io error (including the path being a directory), invalid UTF-8
///   (detected via `String::from_utf8` — deliberately not lossy, which would
///   mask corruption), or blank (all whitespace) → the in-memory shipped
///   default, `fell_back_to_default` true, with the diagnostic labelled
///   `unreadable`, `not valid UTF-8`, or `blank` respectively. Corrupt
///   bytes are NEVER rewritten or healed: they stay on disk as evidence.
pub fn load_persona() -> PersonaLoad {
    let path = persona_path();
    let shown = path.display().to_string();
    match fs::read(&path) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(text) if !text.trim().is_empty() => PersonaLoad {
                document: text,
                seeded_now: false,
                fell_back_to_default: false,
                diagnostic: None,
            },
            Ok(_) => PersonaLoad {
                document: SHIPPED_DEFAULT_PERSONA.to_string(),
                seeded_now: false,
                fell_back_to_default: true,
                diagnostic: Some(format!(
                    "persona file {shown} is blank; serving the shipped default"
                )),
            },
            Err(_) => PersonaLoad {
                document: SHIPPED_DEFAULT_PERSONA.to_string(),
                seeded_now: false,
                fell_back_to_default: true,
                diagnostic: Some(format!(
                    "persona file {shown} is not valid UTF-8; serving the shipped default"
                )),
            },
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            match crate::artifacts::atomic_write(&path, SHIPPED_DEFAULT_PERSONA) {
                Ok(()) => PersonaLoad {
                    document: SHIPPED_DEFAULT_PERSONA.to_string(),
                    seeded_now: true,
                    fell_back_to_default: false,
                    diagnostic: Some(format!(
                        "persona file {shown} was absent; seeded the shipped default"
                    )),
                },
                Err(write_err) => PersonaLoad {
                    document: SHIPPED_DEFAULT_PERSONA.to_string(),
                    seeded_now: false,
                    fell_back_to_default: true,
                    diagnostic: Some(format!(
                        "persona file {shown} was absent but seeding failed: {write_err}"
                    )),
                },
            }
        }
        Err(e) => PersonaLoad {
            document: SHIPPED_DEFAULT_PERSONA.to_string(),
            seeded_now: false,
            fell_back_to_default: true,
            diagnostic: Some(format!("persona file {shown} was unreadable: {e}")),
        },
    }
}

/// Persist `document` verbatim with an atomic rename.
///
/// Preflight rejects blank documents with `InvalidData` before ANY disk
/// touch (the stored file stays byte-identical). Durability delegates to
/// `crate::artifacts::atomic_write`; its anyhow failure maps to an
/// `io::Error` of kind `Other` so callers see plain io results. No BOM
/// handling, no newline translation, no size cap.
pub fn save_persona(document: &str) -> io::Result<()> {
    if document.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "persona document must not be blank",
        ));
    }
    crate::artifacts::atomic_write(&persona_path(), document).map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    // PACKET_HOME is process-global state: whichever test flips it must hold
    // this for its WHOLE body, or siblings' reads land on the wrong home.
    // Sibling env tests (chat_store) flip the same variable under their OWN
    // lock, so in-module serialization alone cannot exclude them; the settle
    // gate below additionally arranges that our claim never straddles a
    // neighbor's (microsecond-scale) body, and the drop tripwire certifies
    // the partition held for the entire critical section. The guard also
    // tears the temp home down (restoring writability first), so
    // parallel-suite siblings always see a writable world.
    static ENV_HOME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Settle-gate cadence: one quiet wait far exceeds any neighboring env-
    /// test body (microseconds of file IO); later rounds escalate the wait
    /// when a sample pair straddles a live transition.
    const SETTLE_WAITS_MS: [u64; 5] = [50, 100, 200, 400, 400];
    const SAMPLE_GAP_MICROS: u64 = 100;

    /// Hold `PACKET_HOME` pointed at a fresh pid-tagged temp dir for the
    /// guard's lifetime; on drop, certify the variable was undisturbed,
    /// restore permissions, and remove the dir.
    struct TmpHome {
        home: PathBuf,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl TmpHome {
        fn new(tag: &str) -> Self {
            let lock = ENV_HOME_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let home =
                env::temp_dir().join(format!("packet_personas_{tag}_{}", std::process::id()));

            // Settle gate: claim only when PACKET_HOME is observably
            // stationary, so a neighbor env test in flight cannot straddle
            // our claim. Escalate per round; after exhaustion fail loudly
            // rather than wedge the suite (drops the lock before panicking).
            for wait_ms in SETTLE_WAITS_MS {
                std::thread::sleep(std::time::Duration::from_millis(wait_ms));
                let before = env::var_os("PACKET_HOME");
                std::thread::sleep(std::time::Duration::from_micros(SAMPLE_GAP_MICROS));
                if before != env::var_os("PACKET_HOME") {
                    continue; // a live transition crossed the sample pair
                }
                let _ = fs::remove_dir_all(&home);
                // SAFETY: guarded by ENV_HOME_LOCK; the settle gate rules
                // out concurrent PACKET_HOME transitions at claim time.
                unsafe { env::set_var("PACKET_HOME", &home) };
                assert_eq!(
                    env::var_os("PACKET_HOME").as_deref(),
                    Some(home.as_os_str()),
                    "PACKET_HOME was overwritten between claim and verify",
                );
                return TmpHome { home, _lock: lock };
            }
            drop(lock);
            panic!("could not observe a settled PACKET_HOME in five escalated waits");
        }
    }

    impl Drop for TmpHome {
        fn drop(&mut self) {
            // Tripwire: a variable disturbed inside the critical section
            // voids this test's filesystem observations. The settle gate
            // should make this unreachable, so a tripped wire demands
            // attention instead of swallowing the race as a pass-or-fail.
            assert_eq!(
                env::var_os("PACKET_HOME").as_deref(),
                Some(self.home.as_os_str()),
                "PACKET_HOME was disturbed during the persona critical section",
            );
            // Reverse any permission mischief before removal (removing a
            // read-only dir's children would otherwise fail), then wipe it.
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&self.home, fs::Permissions::from_mode(0o700));
            }
            let _ = fs::remove_dir_all(&self.home);
        }
    }

    fn persona_bytes() -> Vec<u8> {
        fs::read(persona_path()).expect("persona.md should exist on disk")
    }

    #[test]
    fn first_run_seeds_shipped_default_deterministically() {
        let _home = TmpHome::new("seed");
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
}
