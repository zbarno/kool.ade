//! First-launch / connect screen: pick a git repository, validate it,
//! bootstrap planning artifacts if absent, and hydrate the ~/.packet chat.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use egui::{Frame, RichText, TextEdit};

use crate::app::session::Project;
use crate::core::gitops;
use crate::core::state::PlannerState;
use crate::error::AppError;
use crate::persistence::{chat_store, project_slug};
use crate::ui::theme;

mod repository_picker;

/// Normalize user-typed paths (`~` expansion), then load-or-bootstrap.
pub fn attempt_connect(raw: &str) -> Result<Project, AppError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(AppError::InvalidRepo {
            path: raw.to_string(),
            detail: "Please enter the path to a git repository (working tree).".into(),
        });
    }
    let expanded = expand_home(raw);
    let canonical = std::fs::canonicalize(&expanded).map_err(|e| AppError::InvalidRepo {
        path: raw.to_string(),
        detail: format!("could not open that path: {e}"),
    })?;
    if !canonical.is_dir() {
        return Err(AppError::InvalidRepo {
            path: canonical.to_string_lossy().into_owned(),
            detail: "path is not a directory".into(),
        });
    }
    if !gitops::is_work_tree(&canonical) {
        return Err(AppError::InvalidRepo {
            path: canonical.to_string_lossy().into_owned(),
            detail: "no .git directory found — Packet plans inside a git working tree. Initialize one first (git init) or choose a different folder.".into(),
        });
    }

    crate::artifacts::transaction::recover(&canonical)
        .map_err(|e| AppError::Other(format!("planning transaction recovery failed: {e:#}")))?;
    crate::artifacts::migration::run(&canonical).map_err(|e| AppError::Artifact {
        path: canonical.to_string_lossy().into_owned(),
        detail: format!("project artifact migration failed: {e:#}"),
    })?;
    let mut state = PlannerState::load(&canonical).map_err(|e| AppError::Artifact {
        path: canonical.to_string_lossy().into_owned(),
        detail: e.to_string(),
    })?;
    let created = state.bootstrap_missing().map_err(|e| AppError::Io {
        op: "bootstrap planning artifacts".into(),
        detail: e.to_string(),
    })?;
    let paths = created;
    if !paths.is_empty() {
        gitops::commit(&canonical, "packet: bootstrap project artifacts", &paths).map_err(|e| {
            AppError::Other(format!(
                "bootstrapped artifacts are preserved but checkpoint failed: {e}"
            ))
        })?;
    }
    let slug = project_slug(&canonical);
    let mut chat = chat_store::load(&slug).0;
    if chat.is_empty() {
        let welcome = crate::app::session::welcome_message(&state.title);
        chat.push(welcome.clone());
        chat_store::append(&slug, &[welcome]).ok();
    }
    let task_documents = crate::artifacts::task_docs::load_board(&canonical, &state.workflow);
    let archived_tasks = crate::persistence::archived_tasks::load(&slug);
    let mut project = Project {
        task_chats: Default::default(),
        activity: Default::default(),
        task_documents,
        archived_tasks,
        state,
        chat_slug: slug,
        chat,
        draft: String::new(),
        active_turn: None,
        task_turns: Default::default(),
        task_live: Default::default(),
        planning_work: crate::core::planning_work::load(&canonical)
            .map_err(|e| crate::error::AppError::Other(e.to_string()))?,
        active_planning_work: None,
        queue: crate::core::implementation_queue::Queue::load(&canonical)
            .map_err(|e| AppError::Other(e.to_string()))?,
        queue_lock: None,
        active_implementations: Default::default(),
        pr_refresh: None,
        reconciliation: Default::default(),
        investigation: None,
        investigation_attempted: Default::default(),
        investigation_cooldown_until: None,
        last_pr_refresh: None,
        implementation_states: Default::default(),
        live_progress: crate::harness::LiveProgress::default(),
        next_question_id: None,
        git: gitops::snapshot(&canonical),
    };
    project.refresh_implementations();
    for item in &project.state.items {
        if let Some(activity) = crate::core::implementation::load_activity(&canonical, &item.id) {
            project.activity.tasks.insert(item.id.clone(), activity);
        }
    }
    Ok(project)
}

/// Secondary-button shell for the connect card (Browse… / Clone).
/// With `disabled` true, renders the sunk look — darkest palette fill,
/// muted border, faded text — because egui 0.36 dropped `enabled`/
/// `gray_out` on concrete widgets; callers ALSO gate their `clicked()`
/// checks on `!busy` so the sink is genuinely inert.
fn button_shell(label: &str, disabled: bool) -> egui::Button<'_> {
    if disabled {
        egui::Button::new(RichText::new(label).size(13.0).color(theme::TEXT_DIM))
            .fill(theme::BG)
            .stroke(egui::Stroke::new(1.0, theme::BORDER))
    } else {
        egui::Button::new(RichText::new(label).size(13.0))
    }
}

fn expand_home(raw: &str) -> OsString {
    if let Some(rest) = raw.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest).into_os_string();
    }
    raw.into()
}

// --------------------------------------------------------------------- GitHub URL clone

/// A parsed GitHub clone target.
///
/// `url` is the CANONICAL rebuilt form `https://github.com/{owner}/{repo}.git`
/// — never the raw pasted string — so query/userinfo/host quirks cannot
/// leak into git's argv. Segment case is preserved from what the operator
/// pasted.
pub struct GithubTarget {
    /// Owner as pasted.
    pub owner: String,
    /// Repository as pasted (`.git` suffix and trailing `/` stripped).
    pub repo: String,
    /// `https://github.com/{owner}/{repo}.git` (lowercase scheme + host).
    pub url: String,
}

/// Parse an `https://github.com/{owner}/{repo}` URL the connect card can
/// clone. Std-only (no url/regex crate, per the frozen manifest):
/// trim; require an `https` scheme (case-insensitive); require the
/// authority to be exactly `github.com` (case-insensitive) with NO
/// userinfo, port, query, or fragment anywhere in the remainder; require
/// the remainder to be exactly `/{owner}/{repo}`, optionally terminated by
/// a single `/` or a case-insensitive `.git`; each segment non-empty,
/// `..`-free, ASCII `[A-Za-z0-9._-]`, starting and ending alphanumeric.
///
/// Every rejection is a distinct operator-actionable sentence that names
/// the canonical shape.
pub fn parse_github_url(raw: &str) -> Result<GithubTarget, String> {
    const SHAPE: &str = "https://github.com/{owner}/{repo}";
    const HINT: &str =
        "Paste a github.com URL to clone, e.g. https://github.com/octocat/hello-world";

    let url = raw.trim();
    if url.is_empty() {
        return Err(HINT.to_string());
    }
    // `to_ascii_lowercase` preserves BYTE LENGTH, so indices discovered in
    // the lowered view address the original view 1:1 (segment case stays
    // intact for the rebuilt target).
    let folded_full = url.to_ascii_lowercase();
    let folded: &str = folded_full.as_str();
    let rest = match folded.strip_prefix("https://") {
        Some(rest) => rest,
        None => {
            if folded.starts_with("git@") {
                return Err(format!(
                    "That is the SSH form \u{2014} use the HTTPS one instead: paste {SHAPE} (no git@ prefix)."
                ));
            }
            if let Some(at) = folded.find("://") {
                let scheme: String = folded[..at]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
                    .collect();
                if !scheme.is_empty() {
                    return Err(format!(
                        "Only {SHAPE} URLs can be cloned here (got scheme '{scheme}')."
                    ));
                }
            }
            return Err(HINT.to_string());
        }
    };
    if rest.contains('?') || rest.contains('#') {
        return Err(format!(
            "Strip any \u{2018}?\u{2026}\u{2019} and \u{2018}#\u{2026}\u{2019} extras \u{2014} paste a plain {SHAPE} URL (an optional .git suffix is fine)."
        ));
    }
    // Authority bounds (relative to `rest`) and the path part's absolute
    // start (inclusive of its leading '/', or rest's end when absent).
    let (auth_len, path_at) = match rest.find('/') {
        Some(rel) => (rel, 8 + rel),
        None => (rest.len(), 8 + rest.len()),
    };
    let authority_folded = &rest[..auth_len];
    if authority_folded.is_empty() {
        return Err(format!(
            "Only {SHAPE} URLs can be cloned here (nothing after \u{2018}https://\u{2019} looked like a host)."
        ));
    }
    if authority_folded.contains('@') {
        return Err(format!(
            "Drop the \u{2018}user:token@\u{2019} login \u{2014} paste {SHAPE} plainly (credentials are not supported here)."
        ));
    }
    if authority_folded.contains(':') {
        return Err(format!(
            "Drop the port number \u{2014} GitHub sits on the default HTTPS port, so paste {SHAPE}."
        ));
    }
    if authority_folded != "github.com" {
        return Err(format!(
            "Only {SHAPE} URLs can be cloned here (got {}).",
            &url[8..8 + auth_len]
        ));
    }
    // Trim the path part — ONE leading '/', at most ONE trailing '/', and a
    // terminal case-insensitive ".git" — tracking ABSOLUTE bounds, so the
    // surviving segment range can be re-applied to the ORIGINAL url.
    let mut lo = path_at;
    let mut hi = 8 + rest.len();
    if lo < hi && folded.as_bytes()[lo] == b'/' {
        lo += 1;
    }
    if hi > lo && folded.as_bytes()[hi - 1] == b'/' {
        hi -= 1;
    }
    const DOT_GIT: &str = ".git";
    if hi - lo > DOT_GIT.len() && folded[hi - DOT_GIT.len()..hi].eq_ignore_ascii_case(DOT_GIT) {
        hi -= DOT_GIT.len();
    }
    // Partition on '/' (walked per-char, so every bound is a UTF-8
    // boundary), then validate each part against the segment rules.
    let mut parts: Vec<(usize, usize)> = Vec::new();
    let mut start = lo;
    for (rel_i, ch) in folded[lo..hi].char_indices() {
        if ch == '/' {
            parts.push((start, lo + rel_i));
            start = lo + rel_i + 1;
        }
    }
    parts.push((start, hi));
    if parts.len() == 1 {
        return Err(format!(
            "An owner alone does not name a repository \u{2014} paste {SHAPE}."
        ));
    }
    if parts.len() != 2 {
        return Err(format!(
            "Too many parts \u{2014} paste exactly {SHAPE} (no branch names; an optional .git suffix is fine)."
        ));
    }
    for &(a, b) in &parts {
        if !valid_repo_segment(&folded[a..b]) {
            return Err(format!(
                "Owner and repo names must start and end with a letter or digit, using only letters, digits, \u{2018}-_\u{2019} and dots (no slashes) \u{2014} paste {SHAPE}."
            ));
        }
    }
    // Slice the segments OUT OF THE ORIGINAL URL — operator casing intact.
    let owner = &url[parts[0].0..parts[0].1];
    let repo = &url[parts[1].0..parts[1].1];
    Ok(GithubTarget {
        owner: owner.to_string(),
        repo: repo.to_string(),
        url: format!("https://github.com/{owner}/{repo}.git"),
    })
}

/// `[A-Za-z0-9._-]+`, first and last byte alphanumeric, no `..` runs.
fn valid_repo_segment(segment: &str) -> bool {
    let bytes = segment.as_bytes();
    !bytes.is_empty()
        && !segment.contains("..")
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        && bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
}

/// Where a clone lands: `$HOME/{repo}`. Pure (takes `home`) so tests can
/// inject a sandboxed home; the segment arrives in the operator's original
/// case.
pub fn clone_destination(repo: &str, home: &Path) -> PathBuf {
    home.join(repo)
}

/// Clone `source` (ALWAYS the canonical url rebuilt by
/// [`parse_github_url`]) into `$HOME/{repo}` via the system git CLI.
///
/// Scratch-plus-rename invariant: git writes into
/// `$HOME/{repo}.packet-cloning.<pid>` and the finished tree is renamed
/// into place atomically within $HOME \u{2014} the real destination can
/// never exist half-written, even if the app or the machine dies mid-
/// download. Stale scratch left by crashed attempts for the same repo is
/// swept before every new clone (the sweep predicate and the scratch name
/// stay coupled in this one function).
pub fn perform_clone(source: &str, repo: &str) -> Result<PathBuf, AppError> {
    let home = std::env::var("HOME")
        .ok()
        .filter(|home| !home.trim().is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| AppError::Io {
            op: "locate home directory".into(),
            detail: "Set $HOME to a writable directory before cloning.".into(),
        })?;
    perform_clone_at(source, repo, &home)
}

/// The home-independent core of [`perform_clone`]: destinations, the
/// stale-scratch sweep and the atomic rename all key off `home`, which
/// production resolves from `$HOME` and tests inject directly (no env
/// mutation).
pub fn perform_clone_at(source: &str, repo: &str, home: &Path) -> Result<PathBuf, AppError> {
    let dest = clone_destination(repo, home);
    if dest.is_file() {
        return Err(AppError::InvalidRepo {
            path: dest.to_string_lossy().into_owned(),
            detail: "a file already occupies that name".into(),
        });
    }
    if dest.is_dir() {
        // Already cloned \u{2014} fast-path back through the EXISTING connect
        // path (or let attempt_connect raise its incumbent "no .git
        // directory found" banner for a foreign directory). Never re-clone.
        return Ok(dest);
    }
    if !home.is_dir() {
        return Err(AppError::Io {
            op: "prepare clone parent".into(),
            detail: format!(
                "$HOME points at {}, which is not a directory",
                home.to_string_lossy()
            ),
        });
    }
    let scratch_prefix = format!("{repo}.packet-cloning.");
    if let Ok(entries) = std::fs::read_dir(home) {
        for entry in entries.flatten() {
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with(&scratch_prefix)
            {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
    let scratch = home.join(format!("{repo}.packet-cloning.{}", std::process::id()));
    if let Err(e) = gitops::clone_repo(source, &scratch) {
        let _ = std::fs::remove_dir_all(&scratch);
        return Err(e);
    }
    if !scratch.is_dir() {
        let _ = std::fs::remove_dir_all(&scratch);
        return Err(AppError::Git {
            cmd: crate::error::redact_secrets(&format!("clone {source}")),
            detail: "git reported success but the scratch directory is missing".into(),
        });
    }
    if let Err(e) = std::fs::rename(&scratch, &dest) {
        let _ = std::fs::remove_dir_all(&scratch);
        return Err(AppError::Io {
            op: "place cloned repository".into(),
            detail: format!("could not move the clone into place: {e}"),
        });
    }
    Ok(dest)
}

/// Paint the centered card on the connect screen.
///
/// Raises `*browse_requested` when the 'Browse…' button beside the path
/// field is clicked, and `*clone_requested` when the 'Clone' button is
/// clicked OR Enter is pressed while the GitHub URL field owns focus. The
/// former global Enter hook is retired: it is now focus-scoped, so a bare
/// window-level Enter with no field focused is inert (the Open button
/// covers the mouse path; both fields keep click-to-focus + Tab
/// navigation). While `cloning` is `Some` the card paints the status line,
/// repaints every frame, and swallows ALL input (mirrors story 2's modal
/// discipline). Returning `true` means the existing Open/Enter submit
/// should run.
#[allow(clippy::too_many_arguments)] // card state bag + two script-test field probes
pub fn paint(
    card_ui: &mut egui::Ui,
    path: &mut String,
    github_url: &mut String,
    error: Option<&str>,
    browse_requested: &mut bool,
    clone_requested: &mut bool,
    cloning: Option<(&str, &str)>,
    path_field_probe: Option<&mut (egui::Id, egui::Rect)>,
    url_field_probe: Option<&mut (egui::Id, egui::Rect)>,
) -> bool {
    let busy = cloning.is_some();
    // Snapshot focus BEFORE any field is laid out: single-line TextEdits
    // SURRENDER focus the moment they observe Enter (handled deep inside
    // their own update), so a late query would always read "none" on the
    // very Enter frame that must scope the keystroke. State entering this
    // frame == what the operator perceives as focus at keytime.
    let pre_focus: Option<egui::Id> = card_ui.memory(|m| m.focused());
    card_ui.set_width(460.0);
    card_ui.add_space(12.0);
    card_ui.label(
        RichText::new("Packet")
            .strong()
            .size(36.0)
            .extra_letter_spacing(1.0)
            .color(theme::TEXT),
    );
    card_ui.label(
        RichText::new("Great products start with a clear idea.")
            .weak()
            .size(13.0),
    );
    card_ui.add_space(28.0);
    card_ui.label(
        RichText::new("Open your workspace")
            .size(12.5)
            .color(theme::TEXT_DIM),
    );
    card_ui.add_space(4.0);
    let path_field = card_ui
        .horizontal(|ui| {
            // While the clone runs the row is VISUALLY DISABLED: a dimmed
            // mono read-out stands in for the live editor (truly inert).
            let field = if busy {
                ui.add_sized(
                    egui::vec2(ui.available_width(), 42.0),
                    egui::Label::new(
                        RichText::new(if path.trim().is_empty() {
                            "/path/to/my/project".to_string()
                        } else {
                            path.clone()
                        })
                        .family(egui::FontFamily::Monospace)
                        .size(12.5)
                        .color(theme::TEXT_DIM),
                    ),
                )
            } else {
                ui.add_sized(
                    egui::vec2(ui.available_width(), 42.0),
                    TextEdit::singleline(path)
                        .hint_text("/path/to/my/project")
                        .desired_width(f32::INFINITY)
                        .font(egui::FontId::monospace(12.5)),
                )
            };
            let browse = ui.add_sized(egui::vec2(96.0, 42.0), button_shell("Browse…", busy));
            if !busy && browse.clicked() {
                *browse_requested = true;
            }
            field
        })
        .inner;
    if let Some(p) = path_field_probe {
        p.0 = path_field.id;
        p.1 = path_field.rect;
    }
    repository_picker::paint(card_ui, path);
    card_ui.add_space(6.0);
    let submit_btn = if busy {
        egui::Button::new(
            RichText::new("Open workspace")
                .strong()
                .size(13.0)
                .color(theme::BG),
        )
        .fill(theme::ACCENT_SOFT)
        .corner_radius(6.0)
    } else {
        egui::Button::new(
            RichText::new("Open workspace")
                .strong()
                .size(13.0)
                .color(theme::BG),
        )
        .fill(theme::TEXT)
        .corner_radius(6.0)
    };
    let submit = card_ui.add_sized(egui::vec2(card_ui.available_width(), 44.0), submit_btn);
    if !busy && submit.hovered() {
        card_ui.ctx().request_repaint();
    }
    card_ui.add_space(10.0);
    card_ui.label(
        RichText::new("Or paste a GitHub URL")
            .size(12.5)
            .color(theme::TEXT_DIM),
    );
    card_ui.add_space(4.0);
    let url_field = card_ui
        .horizontal(|ui| {
            let field = if busy {
                ui.add_sized(
                    egui::vec2(ui.available_width(), 42.0),
                    egui::Label::new(
                        RichText::new(if github_url.trim().is_empty() {
                            "https://github.com/{owner}/{repo}".to_string()
                        } else {
                            github_url.clone()
                        })
                        .family(egui::FontFamily::Monospace)
                        .size(12.5)
                        .color(theme::TEXT_DIM),
                    ),
                )
            } else {
                ui.add_sized(
                    egui::vec2(ui.available_width(), 42.0),
                    TextEdit::singleline(github_url)
                        .hint_text("https://github.com/{owner}/{repo}")
                        .desired_width(f32::INFINITY)
                        .font(egui::FontId::monospace(12.5)),
                )
            };
            let clone = ui.add_sized(
                egui::vec2(110.0, 42.0),
                // Disabled while a clone runs AND whenever the trimmed URL
                // is empty (ticket: the control mirrors the path field's
                // empty-silence; begin_clone double-guards anyway).
                button_shell("Clone", busy || github_url.trim().is_empty()),
            );
            if !busy && !github_url.trim().is_empty() && clone.clicked() {
                *clone_requested = true;
            }
            field
        })
        .inner;
    if let Some(p) = url_field_probe {
        p.0 = url_field.id;
        p.1 = url_field.rect;
    }
    if let Some((_label, repo)) = cloning {
        card_ui.add_space(6.0);
        card_ui.label(
            RichText::new(format!("Cloning {repo} from GitHub…"))
                .size(12.5)
                .color(theme::TEXT_DIM),
        );
        card_ui.ctx().request_repaint();
        // Input-stealing while the worker runs: swallow every signal and
        // hand the caller back a clean, actionable-nothing frame.
        *clone_requested = false;
        *browse_requested = false;
        return false;
    }
    if let Some(e) = error {
        Frame::NONE
            .fill(egui::Color32::from_rgb(58, 24, 24))
            .corner_radius(6.0)
            .inner_margin(egui::Margin::symmetric(10, 8))
            .show(card_ui, |ui| {
                ui.label(RichText::new(e).color(theme::DANGER).size(12.5));
            });
        card_ui.add_space(6.0);
    }
    card_ui.add_space(4.0);
    card_ui.label(
        RichText::new(
            "A conversation on the left. A living specification on the right.\nYour decisions, captured and versioned in your repository.",
        )
        .weak()
        .size(11.0),
    );
    // Focus-scoped Enter (the retired global hook's replacement): the PATH
    // field's Enter submits (with the incumbent empty guard); the URL
    // field's Enter requests a clone; any other Enter does nothing.
    let entered =
        card_ui.input(|i| i.key_pressed(egui::Key::Enter)) && card_ui.input(|i| !i.modifiers.ctrl);
    if entered {
        if Some(path_field.id) == pre_focus {
            if !path.trim().is_empty() {
                return true;
            }
        } else if Some(url_field.id) == pre_focus {
            *clone_requested = true;
        }
    }
    submit.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connecting_missing_path_errors_friendly() {
        match attempt_connect("") {
            Ok(_) => panic!("expected connection error"),
            Err(err) => assert!(matches!(err, AppError::InvalidRepo { .. })),
        }
    }

    #[test]
    fn connecting_nonexistent_path_errors_friendly() {
        match attempt_connect("/no/such/dir-xyz-123") {
            Ok(_) => panic!("expected connection error"),
            Err(err) => assert!(matches!(err, AppError::InvalidRepo { .. })),
        }
    }

    // ---- parse_github_url --------------------------------------------------

    #[test]
    fn parse_github_url_accepts_canonical_forms() {
        let cases = [
            (
                "https://github.com/octocat/hello-world",
                "octocat",
                "hello-world",
            ),
            (
                "https://github.com/octocat/hello-world.git",
                "octocat",
                "hello-world",
            ),
            (
                "https://github.com/octocat/hello-world/",
                "octocat",
                "hello-world",
            ),
            (
                "https://github.com/octocat/hello-world.git/",
                "octocat",
                "hello-world",
            ),
            (
                "  https://github.com/octocat/hello-world  ",
                "octocat",
                "hello-world",
            ),
        ];
        for (raw, owner, repo) in cases {
            let t = parse_github_url(raw).unwrap_or_else(|e| panic!("{raw:?} was rejected: {e}"));
            assert_eq!(t.owner, owner, "{raw:?}");
            assert_eq!(t.repo, repo, "{raw:?}");
            assert_eq!(
                t.url, "https://github.com/octocat/hello-world.git",
                "rebuilt canonical: {raw:?}"
            );
        }
    }

    #[test]
    fn parse_github_url_preserves_segment_case_and_rebuilds_canonical() {
        let t = parse_github_url("HTTPS://GITHUB.COM/Octo/Repo.GIT").expect("mixed-case accepted");
        assert_eq!(t.owner, "Octo", "original segment case kept");
        assert_eq!(t.repo, "Repo");
        assert_eq!(t.url, "https://github.com/Octo/Repo.git");

        let t = parse_github_url("HtTpS://GiThUb.CoM/A.B-C_D/x_y.z-9").expect("accepted");
        assert_eq!(t.owner, "A.B-C_D");
        assert_eq!(t.repo, "x_y.z-9");
        assert_eq!(t.url, "https://github.com/A.B-C_D/x_y.z-9.git");
    }

    #[test]
    fn parse_github_url_rejects_each_quirk_with_distinct_actionable_guidance() {
        const SHAPE: &str = "https://github.com/{owner}/{repo}";
        let rejected: &[(&str, &str)] = &[
            ("", "e.g. https://github.com/octocat/hello-world"),
            ("   ", "e.g. https://github.com/octocat/hello-world"),
            ("notaurl", "e.g. https://github.com/octocat/hello-world"),
            ("ftp://github.com/o/r", "got scheme 'ftp'"),
            ("http://github.com/o/r", "got scheme 'http'"),
            ("ssh://git@github.com/o/r", "got scheme 'ssh'"),
            ("git@github.com:o/r.git", "SSH form"),
            ("https://gitee.com/o/r", "got gitee.com"),
            ("https://githubcorp.com/o/r", "got githubcorp.com"),
            ("https://user:token@github.com/o/r", "login"),
            ("https://github.com:443/o/r", "port number"),
            ("https://github.com/o/r?ref=1", "extras"),
            ("https://github.com/o/r#anchor", "extras"),
            ("https://github.com/o", "owner alone"),
            ("https://github.com", "owner alone"),
            ("https://github.com/o/r/branch", "Too many parts"),
            ("https://github.com/o/r//", "Too many parts"),
            (
                "https://github.com/../r",
                "start and end with a letter or digit",
            ),
            (
                "https://github.com/o/..",
                "start and end with a letter or digit",
            ),
            (
                "https://github.com/lead-/r",
                "start and end with a letter or digit",
            ),
            (
                "https://github.com/o/trail-",
                "start and end with a letter or digit",
            ),
            (
                "https://github.com/caf\u{e9}/r",
                "start and end with a letter or digit",
            ),
            (
                "https://github.com/./r",
                "start and end with a letter or digit",
            ),
            ("https:///o/r", "looked like a host"),
        ];
        for (raw, needle) in rejected {
            let err = parse_github_url(raw)
                .err()
                .unwrap_or_else(|| panic!("{raw:?} was accepted"));
            assert!(
                err.contains(needle),
                "{raw:?}: `{err}` lacks guidance `{needle}`"
            );
            assert!(
                err.contains(SHAPE) || err.contains("Paste a github.com URL"),
                "{raw:?}: `{err}` does not name the canonical shape"
            );
        }
    }

    // ---- clone_destination -------------------------------------------------

    #[test]
    fn clone_destination_joins_home_and_repo_verbatim() {
        assert_eq!(
            clone_destination("My-Repo", Path::new("/Users/op")),
            PathBuf::from("/Users/op/My-Repo")
        );
        assert_eq!(
            clone_destination("a.b-c_d", Path::new("/srv/repos")),
            PathBuf::from("/srv/repos/a.b-c_d")
        );
    }

    // ---- perform_clone (OFFLINE: local file-path git sources) --------------
    //
    // Environment-mutation discipline (house convention, cf. gitops'
    // test_support::shield): the HOUSE GIT_HIERARCHY_LOCK shield held for
    // the WHOLE test body — every other env-mutating suite member (glue,
    // feature_approval, root) takes the same lock, so windows never
    // interleave; HOME and PACKET_HOME restored on drop.

    struct EnvSandbox {
        _shield: crate::core::gitops::test_support::Guard,
        prev_home: Option<std::ffi::OsString>,
        prev_packet_home: Option<std::ffi::OsString>,
        home: PathBuf,
    }

    impl EnvSandbox {
        fn enter(tag: &str, drop_home: bool) -> Self {
            let _shield = crate::core::gitops::test_support::shield(tag);
            let home = std::env::temp_dir()
                .join(format!("packet_swclone_home_{tag}_{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&home);
            std::fs::create_dir_all(&home).unwrap();
            let packet_home = home.join("packet-state");
            std::fs::create_dir_all(&packet_home).unwrap();
            let prev_home = std::env::var_os("HOME");
            let prev_packet_home = std::env::var_os("PACKET_HOME");
            // SAFETY: the house shield is held for this whole body; no
            // sibling test mutates or depends on HOME/PACKET_HOME while it
            // is held.
            unsafe {
                if drop_home {
                    std::env::remove_var("HOME");
                } else {
                    std::env::set_var("HOME", &home);
                }
                std::env::set_var("PACKET_HOME", &packet_home);
            }
            Self {
                _shield,
                prev_home,
                prev_packet_home,
                home,
            }
        }
    }

    impl Drop for EnvSandbox {
        fn drop(&mut self) {
            // SAFETY: the shield still guards (its field drops after this
            // body), so the restore cannot interleave with any sibling.
            unsafe {
                match &self.prev_home {
                    Some(value) => std::env::set_var("HOME", value),
                    None => std::env::remove_var("HOME"),
                }
                match &self.prev_packet_home {
                    Some(value) => std::env::set_var("PACKET_HOME", value),
                    None => std::env::remove_var("PACKET_HOME"),
                }
            }
            let _ = std::fs::remove_dir_all(&self.home);
        }
    }

    /// Vanilla GitHub-shaped source: an init -b main tree, LOCAL identity,
    /// one commit, and DELIBERATELY no planning/ directory — the cold-
    /// start shape a fresh clone of an upstream repository brings in.
    fn local_source_repo(tag: &str) -> PathBuf {
        let p =
            std::env::temp_dir().join(format!("packet_swclone_src_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        let git = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .arg("-C")
                .arg(&p)
                .args(args)
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?} failed");
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["config", "user.name", "Clone Src"]);
        git(&["config", "user.email", "src@example.invalid"]);
        std::fs::write(p.join("README.md"), "# Cloned source\n").unwrap();
        git(&["add", "README.md"]);
        git(&["commit", "-q", "-m", "initial"]);
        p
    }

    fn no_scratch_left(home: &Path) {
        let offenders: Vec<_> = std::fs::read_dir(home)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains("packet-cloning"))
            .collect();
        assert!(offenders.is_empty(), "scratch remnants: {offenders:?}");
    }

    /// Fresh throwaway home directory (NO env involvement at all —
    /// [`super::perform_clone_at`] takes it by reference).
    fn scratch_home(tag: &str) -> PathBuf {
        let p =
            std::env::temp_dir().join(format!("packet_swclone_home_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn perform_clone_local_source_feeds_the_full_connect_pipeline() {
        // CHAIN PROOF (ticket plan 5 / AC5): perform_clone →
        // attempt_connect yields a hydrated Project from a PLANNING-LESS
        // source: validation, transaction recovery, state load, scaffold
        // bootstrap (product modules + open-items + config), checkpoint,
        // chat hydration and queue load all run exactly as for
        // a typed path — because it IS the same call. HOME + PACKET_HOME
        // point at throwaways for the whole body and are restored on drop;
        // the house shield keeps sibling git spawns on a consistent
        // identity hierarchy.
        let sb = EnvSandbox::enter("chain", false);
        let src = local_source_repo("chain"); // no planning/ — cold start

        let dest = perform_clone(src.to_str().unwrap(), "sw-clone")
            .expect("offline local-source clone lands");
        assert_eq!(
            dest,
            sb.home.join("sw-clone"),
            "$HOME joined with the repo segment verbatim"
        );
        assert!(gitops::is_work_tree(&dest), "the clone is a working tree");
        assert!(dest.join("README.md").exists(), "files arrived");
        no_scratch_left(&sb.home);

        let project = attempt_connect(dest.to_str().unwrap())
            .unwrap_or_else(|e| panic!("the clone must connect like a typed path: {e:?}"));
        assert_eq!(project.state.title, "sw-clone");
        // A new repository receives the modular scaffold directly. Legacy
        // archives are created only when there was an old specification to
        // preserve.
        assert!(
            dest.join(crate::artifacts::product_docs::INDEX).exists(),
            "cold bootstrap landed the product modules"
        );
        assert!(
            !dest.join("planning/specification.md").exists(),
            "cold bootstrap does not create a legacy specification"
        );
        assert!(
            dest.join(crate::artifacts::OPEN_ITEMS_FILE).exists(),
            "bootstrap_missing created the open-items file"
        );
        assert!(
            dest.join(crate::artifacts::CONFIG_FILE).exists(),
            "bootstrap_missing created the config file"
        );
        // Hydration: the welcome line is in memory AND appended to the
        // chat store UNDER the redirected PACKET_HOME, keyed by slug.
        assert!(!project.chat.is_empty(), "welcome chat hydrated in memory");
        let slug = crate::persistence::project_slug(&dest);
        let jsonl = crate::persistence::project_dir(&slug).join("chat.jsonl");
        let stored = std::fs::read_to_string(&jsonl).unwrap_or_else(|e| {
            panic!("welcome chat line missing under redirected PACKET_HOME ({jsonl:?}): {e}")
        });
        assert!(
            stored.contains("Connected to \u{201c}sw-clone\u{201d}"),
            "welcome line stored: {stored}"
        );
        // Loaded-project population: the git snapshot reflects the bootstrap
        // checkpoint lineage (clean tree, main branch) and the queue loader
        // ran without error.
        assert_eq!(project.git.branch, "main", "snapshot branch populated");
        assert_eq!(
            project.git.dirty, 0,
            "the checkpoint adopted every artifact"
        );
        assert!(
            project.git.last_subject.contains("bootstrap"),
            "last_subject: {}",
            project.git.last_subject
        );
        assert!(project.queue.auto_build, "queue loaded without error");
        let _ = std::fs::remove_dir_all(&src);
    }

    #[test]
    fn migrate_checkpoint_still_stages_tracked_legacy_deletions() {
        // No-regression anchor for the ghost-pathspec filter in
        // attempt_connect: when the legacy spec WAS tracked at HEAD (the
        // everyday evolving-repo shape), the migration checkpoint must
        // still RECORD its deletion and the archive arrival — the filter
        // drops only paths git never knew, never stageable deletions.
        let src =
            std::env::temp_dir().join(format!("packet_swclone_tracksrc_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&src);
        std::fs::create_dir_all(src.join("planning")).unwrap();
        let git = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .arg("-C")
                .arg(&src)
                .args(args)
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?} failed");
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["config", "user.name", "Track Src"]);
        git(&["config", "user.email", "track@example.invalid"]);
        git(&["config", "commit.gpgsign", "false"]);
        std::fs::write(src.join("README.md"), "# tracked legacy\n").unwrap();
        std::fs::write(
            src.join("planning/specification.md"),
            crate::artifacts::spec_doc::bootstrap_template("Track Site"),
        )
        .unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "legacy spec tracked at HEAD"]);

        let _sb = EnvSandbox::enter("tracked", false);
        let dest = perform_clone(src.to_str().unwrap(), "sw-track").expect("clone lands");
        let project = attempt_connect(dest.to_str().unwrap())
            .unwrap_or_else(|e| panic!("tracked-legacy connect: {e:?}"));
        assert_eq!(project.state.title, "sw-track");
        assert!(dest.join(crate::artifacts::product_docs::INDEX).exists());
        assert!(
            dest.join(crate::artifacts::product_docs::LEGACY_ARCHIVE)
                .exists()
        );
        assert!(
            dest.join(".kool-ade-packet/planning/product/index.md")
                .exists()
        );
        assert!(!dest.join("planning/specification.md").exists());

        // Migration checkpoints the deletion + archive arrival before the
        // separate cold-start bootstrap checkpoint adds config and queue files.
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(&dest)
            .args(["log", "-2", "--name-status"])
            .output()
            .unwrap();
        let log_txt = String::from_utf8_lossy(&out.stdout).to_string();
        // Default rename detection may express the move as R(from→archive)
        // rather than D+A — accept either expression; the archive side
        // and the legacy source side must both be present.
        let mut legacy_seen = false;
        let mut archive_seen = false;
        for line in log_txt.lines() {
            if line.starts_with("R") || line.starts_with("D") || line.starts_with("A") {
                if line.split('\t').nth(1) == Some("planning/specification.md") {
                    legacy_seen = true;
                }
                if line
                    .split('\t')
                    .nth(1)
                    .is_some_and(|t| t.ends_with("archive/specification-pre-modules.md"))
                    || line.contains("planning/archive/specification-pre-modules.md")
                {
                    archive_seen = true;
                }
            }
        }
        assert!(legacy_seen, "legacy spec departure staged: {log_txt}");
        assert!(archive_seen, "archive arrival staged: {log_txt}");
        let _ = std::fs::remove_dir_all(&src);
    }

    #[test]
    fn perform_clone_healthy_existing_tree_fast_paths_without_retouching() {
        // AC6: $HOME/{repo} already a HEALTHY work tree from a previous
        // clone → immediate return, no re-clone, no scratch remnant, and
        // not even a new commit on the tree.
        let home = scratch_home("health");
        let src = local_source_repo("health");
        let dest = home.join("sw-health");
        std::fs::create_dir_all(&dest).unwrap();
        let git = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .arg("-C")
                .arg(&dest)
                .args(args)
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?} failed");
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["config", "user.name", "Health"]);
        git(&["config", "user.email", "h@example.invalid"]);
        std::fs::write(dest.join("note.txt"), b"frozen-bytes").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "healthy head"]);

        let got =
            perform_clone_at(src.to_str().unwrap(), "sw-health", &home).expect("fast path returns");
        assert_eq!(got, dest);
        assert_eq!(
            std::fs::read(dest.join("note.txt")).unwrap(),
            b"frozen-bytes",
            "no re-clone: tree byte-identical"
        );
        assert!(gitops::is_work_tree(&dest));
        let head = std::process::Command::new("git")
            .arg("-C")
            .arg(&dest)
            .args(["log", "-1", "--format=%s"])
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&head.stdout),
            "healthy head\n",
            "no checkpoint of any kind was added"
        );
        no_scratch_left(&home);
        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(&src);
    }

    #[test]
    fn perform_clone_plain_repo_connects_identically_to_a_typed_path() {
        // AC5'S identity clause made observable: the SAME planning-less
        // tree, reached once THROUGH the clone and once by hand-typing
        // the source's path, completes with side-effect parity — both
        // bootstrap the scaffold and checkpoint it in both paths, both
        // hydrate chat. The clone feeds EXACTLY today's connect path
        // because it literally is that call.
        let _sb = EnvSandbox::enter("plain", false);
        let src = local_source_repo("plain-src"); // planning-less
        let src_title = src.file_name().unwrap().to_str().unwrap().to_string();

        // Clone leg first, while `src` is still pristine.
        let dest = perform_clone(src.to_str().unwrap(), "sw-plain")
            .expect("offline local-source clone lands");
        assert!(gitops::is_work_tree(&dest));
        let proj_clone = attempt_connect(dest.to_str().unwrap())
            .unwrap_or_else(|e| panic!("cloned planning-less repo must connect: {e:?}"));

        // Typed leg: the operator opens the SAME source by hand.
        let proj_typed = attempt_connect(src.to_str().unwrap())
            .unwrap_or_else(|e| panic!("typed planning-less repo must connect identically: {e:?}"));

        assert_eq!(proj_clone.state.title, "sw-plain");
        assert_eq!(
            proj_typed.state.title, src_title,
            "titles follow the dir names"
        );
        for (root, what) in [(dest.as_path(), "clone leg"), (src.as_path(), "typed leg")] {
            assert!(
                root.join(crate::artifacts::product_docs::INDEX).exists(),
                "{what}: product modules landed"
            );
            assert!(
                !root.join("planning/specification.md").exists(),
                "{what}: cold bootstrap does not create a legacy spec"
            );
            assert!(
                root.join(crate::artifacts::OPEN_ITEMS_FILE).exists(),
                "{what}: open-items bootstrapped"
            );
            assert!(
                root.join(crate::artifacts::CONFIG_FILE).exists(),
                "{what}: config bootstrapped"
            );
        }
        assert!(
            proj_clone
                .chat
                .last()
                .is_some_and(|m| m.text.contains("Connected to")),
            "clone leg hydrated the welcome line"
        );
        assert!(
            proj_typed
                .chat
                .last()
                .is_some_and(|m| m.text.contains("Connected to")),
            "typed leg hydrated the same welcome line"
        );
        let _ = std::fs::remove_dir_all(&src);
    }

    #[test]
    fn perform_clone_preexisting_directory_fast_paths_and_connect_diagnoses_it() {
        let home = scratch_home("skip");
        let src = local_source_repo("skip");
        let dest = home.join("sw-skip");
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::write(dest.join("marker.bin"), b"frozen-bytes").unwrap();

        let got =
            perform_clone_at(src.to_str().unwrap(), "sw-skip", &home).expect("fast path returns");
        assert_eq!(got, dest);
        assert_eq!(
            std::fs::read(dest.join("marker.bin")).unwrap(),
            b"frozen-bytes",
            "no re-clone: the pre-existing directory is byte-identical"
        );
        no_scratch_left(&home);

        // A FOREIGN (non-git) directory is diagnosed by the INCUMBENT
        // attempt_connect banner — no invented new logic.
        match attempt_connect(dest.to_str().unwrap()) {
            Err(err) => {
                let msg = format!("{} | {}", err.headline(), err.detail());
                assert!(
                    msg.contains("no .git directory found"),
                    "legacy banner: {msg}"
                );
            }
            Ok(_) => panic!("a foreign (non-git) directory must be rejected"),
        }
        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(&src);
    }

    #[test]
    fn perform_clone_sweeps_stale_scratch_before_cloning() {
        let home = scratch_home("sweep");
        let src = local_source_repo("sweep");
        let stale = home.join("sw-sweep.packet-cloning.999");
        std::fs::create_dir_all(stale.join("junk")).unwrap();
        std::fs::write(stale.join("junk/leftover.txt"), "dead").unwrap();

        let dest = perform_clone_at(src.to_str().unwrap(), "sw-sweep", &home).expect("clone lands");
        assert!(!stale.exists(), "stale foreign-pid scratch swept");
        assert!(gitops::is_work_tree(&dest));

        // Same-pid repetition: the sweep must also clear THIS process'
        // scratch name, and the second clone rides the same scratch path.
        let own = home.join(format!("sw-sweep.packet-cloning.{}", std::process::id()));
        std::fs::remove_dir_all(&dest).unwrap();
        std::fs::create_dir_all(&own).unwrap();
        let again =
            perform_clone_at(src.to_str().unwrap(), "sw-sweep", &home).expect("reclone lands");
        assert!(!own.exists(), "same-pid scratch swept");
        assert!(again.is_dir());
        no_scratch_left(&home);
        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(&src);
    }

    #[test]
    fn perform_clone_destination_occupied_by_a_file_is_invalid_repo() {
        let home = scratch_home("fileocc");
        let src = local_source_repo("fileocc");
        std::fs::write(home.join("sw-fileocc"), "blocker").unwrap();

        match perform_clone_at(src.to_str().unwrap(), "sw-fileocc", &home) {
            Err(AppError::InvalidRepo { path, detail }) => {
                assert!(path.ends_with("sw-fileocc"));
                assert!(
                    detail.contains("a file already occupies that name"),
                    "{detail}"
                );
            }
            other => panic!("expected InvalidRepo, got {other:?}"),
        }
        assert!(
            home.join("sw-fileocc").is_file(),
            "the blocker file survived"
        );
        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(&src);
    }

    #[test]
    fn perform_clone_without_home_errors_locate_home() {
        let _sb = EnvSandbox::enter("nohome", true); // HOME removed for the body
        match perform_clone("https://github.com/o/r.git", "sw-nohome") {
            Err(AppError::Io { op, detail }) => {
                assert!(op.contains("locate home directory"), "{op}");
                assert!(detail.to_lowercase().contains("$home"), "{detail}");
            }
            other => panic!("expected Io locate-home, got {other:?}"),
        }
    }
}
