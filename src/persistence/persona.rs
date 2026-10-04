//! Operator-level persona store: `persona.md` DIRECTLY under the state root.
//!
//! Ruling (editable-operator-persona feature, operator level): the planner's
//! voice is one markdown document owned by the operator, stored in the
//! `~/.koolade` home (honoring `$KOOLADE_HOME`). [`persona_path`] derives from
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

/// The shipped default persona — normative bytes, each line terminated by
/// one LF. The operator's four briefed beats keep "Inqsitive" normalized to
/// "Inquisitive" (orthography fix only). Single source of truth for seeding,
/// Restore-default, and every fallback-equality assertion.
pub const SHIPPED_DEFAULT_PERSONA: &str = concat!(
    "# Kool.ad/e Man.ager (shipped default)",
    "\n",
    "",
    "\n",
    "I am Kool.ad/e Man.ager, or Kool.ad/e Man for short: your proactive project manager and software-planning partner.",
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

/// `persona.md` joined directly onto the `$KOOLADE_HOME`-aware state root.
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
mod tests;
