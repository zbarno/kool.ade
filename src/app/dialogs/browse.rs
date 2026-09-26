use super::*;

/// Pure form of [`expand_tilde`]: expand a leading `~/` against the
/// supplied home directory (no environment access).
fn expand_tilde_against(raw: &str, home: Option<&str>) -> String {
    if let (Some(rest), Some(home)) = (raw.strip_prefix("~/"), home) {
        return format!("{home}/{rest}");
    }
    raw.to_string()
}

pub(super) fn expand_tilde(raw: &str) -> String {
    expand_tilde_against(raw, std::env::var("HOME").ok().as_deref())
}

// ---------------------------------------------------------------------------
// Workspace folder browser (initial-screen “Browse…” button)
// ---------------------------------------------------------------------------

/// One selectable line of the browser listing.
pub(super) struct DirRow {
    pub(super) path: PathBuf,
    pub(super) up: bool,  // synthetic ".." row, never a real subdirectory
    pub(super) git: bool, // live `gitops::is_work_tree` mark (memoized in `DlgBrowse`)
}

/// Directory browser seeded from the connect screen's path field.
///
/// Writes nothing itself: the caller copies [`DlgBrowse::selection`] into
/// `PacketApp::conn_path`. Whether a folder is an acceptable workspace stays
/// entirely with `welcome::attempt_connect` — the browser marks git working
/// trees green but rejects nothing (uninitialized and non-git folders remain
/// choosable, reproducing today's InvalidRepo banner only on Open).
pub struct DlgBrowse {
    pub(super) current: PathBuf,
    pub(super) selected: PathBuf,
    pub(super) rows: Vec<DirRow>,
    pub(super) read_error: Option<String>,
    pub(super) git_cache: HashMap<PathBuf, bool>,
}

impl DlgBrowse {
    /// Seed from the operator's current path field: an existing directory (or
    /// its closest surviving ancestor), else `$HOME`, else `/` — see
    /// [`resolve_seed`].
    pub fn seeded(seed: String) -> Self {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let (current, selected) = resolve_seed(&seed, home.as_deref(), Path::new("/"));
        let mut dlg = Self {
            current,
            selected,
            rows: Vec::new(),
            read_error: None,
            git_cache: HashMap::new(),
        };
        dlg.refresh_rows();
        dlg
    }

    /// The chosen folder. Always an existing directory, so "Choose folder"
    /// is valid from frame one.
    pub fn selection(&self) -> &Path {
        &self.selected
    }

    /// Reload the listing of `current`:
    /// subdirectories only (symlinks followed), dot-dirs INCLUDED (deliberate
    /// v1 presentation: no exclusion rule), sorted case-insensitively by
    /// name, a synthetic up row PREPENDed unless we stand at the filesystem
    /// root (or a symlink loop would cycle). Each subdirectory gets ONE live
    /// `gitops::is_work_tree` probe, memoized by absolute path for the
    /// dialog's life — never probed per paint. Unreadable directories degrade
    /// to a dim notice (`read_error`) with an empty list instead of panicking.
    pub(crate) fn refresh_rows(&mut self) {
        self.rows.clear();
        self.read_error = None;
        let entries = match std::fs::read_dir(&self.current) {
            Ok(rd) => rd.filter_map(Result::ok).collect::<Vec<_>>(),
            Err(e) => {
                self.read_error = Some(e.to_string());
                Vec::new()
            }
        };
        let mut dirs: Vec<PathBuf> = Vec::new();
        for entry in entries {
            let p = entry.path();
            if p.is_dir() {
                dirs.push(p);
            }
        }
        dirs.sort_by(|a, b| {
            cmp_names_ci(
                a.file_name().and_then(|n| n.to_str()).unwrap_or(""),
                b.file_name().and_then(|n| n.to_str()).unwrap_or(""),
            )
        });
        for p in dirs {
            // Canonicalize gives the cache a stable key and drops links that
            // rotted between listing and use.
            let Ok(abs) = std::fs::canonicalize(&p) else {
                continue;
            };
            let git = *self
                .git_cache
                .entry(abs.clone())
                .or_insert_with(|| gitops::is_work_tree(&abs));
            self.rows.push(DirRow {
                path: abs,
                up: false,
                git,
            });
        }
        if let Some(up) = up_landing(&self.current) {
            self.rows.insert(
                0,
                DirRow {
                    path: up,
                    up: true,
                    git: false,
                },
            );
        }
    }

    /// Move the listing into `target`; on success `selected` re-anchors to
    /// the new current. A failed navigation (folder vanished mid-flight) is
    /// a no-op aside from re-degrading the listing.
    pub(crate) fn descend_into(&mut self, target: PathBuf) {
        let Ok(next) = std::fs::canonicalize(&target) else {
            self.refresh_rows();
            return;
        };
        if !next.is_dir() || next == self.current {
            // Already viewing it (Enter confirms): selected := current.
            self.selected = self.current.clone();
            self.refresh_rows();
            return;
        }
        self.current = next;
        self.selected = self.current.clone();
        self.refresh_rows();
    }

    /// Ascend to the parent folder (the up row); `selected` := new current.
    /// Terminates at the filesystem root; the canonical-equality guard also
    /// kills symlinked-parent cycles.
    pub(crate) fn ascend(&mut self) {
        let Some(parent) = self.current.parent().map(Path::to_path_buf) else {
            return;
        };
        let Ok(next) = std::fs::canonicalize(&parent) else {
            return;
        };
        if next == self.current {
            return;
        }
        self.current = next;
        self.selected = self.current.clone();
        self.refresh_rows();
    }
}

/// Case-insensitive name order with the raw name as a deterministic
/// tiebreak (equal stems are impossible in one directory, but two entries
/// may still compare equal when lowercased on odd locales).
fn cmp_names_ci(a: &str, b: &str) -> std::cmp::Ordering {
    a.to_lowercase()
        .cmp(&b.to_lowercase())
        .then_with(|| a.cmp(b))
}

/// Parent landing spot for the up row: `Some(canonical(parent))` when the
/// parent exists and differs from `current`'s own canonical form (omitted at
/// `/` and inside symlink loops). If `current` vanished, the parent still
/// lands the operator somewhere readable.
fn up_landing(current: &Path) -> Option<PathBuf> {
    let parent = current.parent()?;
    let parent_canon = std::fs::canonicalize(parent).ok()?;
    let differs = match std::fs::canonicalize(current) {
        Ok(current_canon) => current_canon != parent_canon,
        Err(_) => true,
    };
    differs.then_some(parent_canon)
}

/// Pure seed-resolution ladder (unit-tested without touching the
/// environment):
/// the trimmed, `~`-expanded seed (against `home`)
/// (a) exists as a dir → canonicalize it;
/// (b) else its parent exists as a dir → canonicalize the parent;
/// (c) else `home`, when set and a dir → canonicalize it;
/// (d) else canonicalize `root` (the app passes `/`).
/// Every outcome is `(existing_directory, the_same)`, so the browser opens
/// on a directory where Choose is already valid.
pub(crate) fn resolve_seed(seed: &str, home: Option<&Path>, root: &Path) -> (PathBuf, PathBuf) {
    let expanded = expand_tilde_against(seed.trim(), home.and_then(Path::to_str));
    let candidate = Path::new(expanded.as_str());
    let fallback = home
        .filter(|h| h.is_dir())
        .and_then(|h| h.canonicalize().ok())
        .or_else(|| root.canonicalize().ok());
    let resolved = if candidate.is_dir() {
        candidate.canonicalize().ok()
    } else if candidate.is_file() {
        // A loose FILE: land in its parent directory.
        candidate.parent().and_then(|p| p.canonicalize().ok())
    } else {
        None
    }
    .or(fallback);
    match resolved {
        Some(dir) => (dir.clone(), dir),
        // Defensive: only reachable when canonicalizing `/` itself fails.
        None => (root.to_path_buf(), root.to_path_buf()),
    }
}

/// Paint the browse card body; returns `(choose_pressed, cancel_pressed)`,
/// the `(save, close)` tuple convention of the house cards. Single click
/// SELECTS; double click (or Enter, which re-confirms the current folder)
/// DESCENDS; the up row ascends. No button here initiates a connect.
pub fn paint_browse_card(ui: &mut egui::Ui, dlg: &mut DlgBrowse) -> (bool, bool) {
    // Cheap liveness probe: if `current` ceased to exist while the modal sat
    // open, re-degrade once to the "cannot read" notice.
    if !dlg.current.is_dir() {
        dlg.refresh_rows();
    }

    ui.label(
        RichText::new("Directories only. Green names sit inside a git working tree.")
            .weak()
            .size(11.0),
    );
    ui.add_space(6.0);

    // Swap the rows out so navigation inside the loop can reborrow `dlg`.
    // Remembers the view so a mid-loop descent/ascend (which re-lists into
    // `dlg.rows`) is not clobbered by restoring the stale snapshot.
    let view_before = dlg.current.clone();
    let rows = std::mem::take(&mut dlg.rows);
    for row in &rows {
        let name = if row.up {
            String::from("..")
        } else {
            row.path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| row.path.display().to_string())
        };
        let label = RichText::new(name).color(if row.git { theme::SUCCESS } else { theme::TEXT });
        let response = ui.selectable_label(dlg.selected == row.path, label);
        if response.double_clicked() {
            if row.up {
                dlg.ascend();
            } else {
                dlg.descend_into(row.path.clone());
            }
        } else if response.clicked() {
            dlg.selected = row.path.clone();
        }
    }
    // If the loop navigated, `dlg.rows` already holds the fresh listing —
    // putting the pre-navigation snapshot back would freeze the view on the
    // old directory. Otherwise restore the snapshot verbatim.
    if dlg.current == view_before {
        dlg.rows = rows;
    }

    if ui.ctx().input(|i| i.key_pressed(egui::Key::Enter)) {
        dlg.descend_into(dlg.current.clone());
    }

    if let Some(error) = &dlg.read_error {
        ui.add_space(4.0);
        ui.label(
            RichText::new(format!("Cannot read this folder: {error}"))
                .weak()
                .size(11.0)
                .color(theme::TEXT_DIM),
        );
    }

    ui.add_space(10.0);
    let full = dlg.selected.to_string_lossy().into_owned();
    ui.add(
        egui::Label::new(
            RichText::new(full.clone())
                .font(egui::FontId::monospace(12.5))
                .color(theme::TEXT_DIM),
        )
        .truncate(),
    )
    .on_hover_text(full);
    ui.add_space(10.0);

    let can_choose = dlg.selected.exists() && dlg.selected.is_dir();
    let mut choose = false;
    let mut cancel = false;
    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
        let choose_btn = ui.add_enabled(
            can_choose,
            egui::Button::new(RichText::new("Choose folder").strong().color(theme::BG))
                .fill(theme::ACCENT_SOFT)
                .corner_radius(6.0),
        );
        if choose_btn.clicked() {
            choose = true;
        }
        if ui.button(RichText::new("Cancel").weak()).clicked() {
            cancel = true;
        }
        ui.add_space(4.0);
    });
    (choose, cancel)
}
