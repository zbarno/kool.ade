use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Component, Path};

use crate::core::repo_overview::Overview;

const MAX_AREAS: usize = 60;
const MAX_FILES_PER_AREA: usize = 8;
const MAX_AREA_CHARS: usize = 16_000;
const MAX_FILE_CHARS: usize = 6_000;
const SOURCE_EXTENSIONS: &[&str] = &[
    "c", "cc", "cpp", "cs", "go", "h", "hpp", "java", "js", "jsx", "kt", "md", "mjs", "py", "rs",
    "swift", "ts", "tsx",
];
const SKIP_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    "target",
    "node_modules",
    "vendor",
    "dist",
    "build",
    "out",
    "coverage",
    "__pycache__",
    ".venv",
    "venv",
    ".idea",
    ".vscode",
    ".next",
    ".turbo",
    ".cache",
];

pub(super) fn catalog(root: &Path, overview: &Overview) -> BTreeMap<String, String> {
    let mut areas = BTreeMap::new();
    for line in &overview.tree_lines {
        let Some(area) = line.strip_suffix('/') else {
            continue;
        };
        if !safe_relative(area) || skipped(area) {
            continue;
        }
        let path = root.join(area);
        if !real_directory(&path) || !contained(root, &path) {
            continue;
        }
        let files = list_source_names(&path, 0, MAX_FILES_PER_AREA);
        if !files.is_empty() {
            areas.insert(area.to_owned(), files.join(", "));
        }
        if areas.len() == MAX_AREAS {
            break;
        }
    }
    areas
}

pub(super) fn load(area: &str, catalog: &BTreeMap<String, String>, root: &Path) -> Option<String> {
    if !catalog.contains_key(area) || !safe_relative(area) || skipped(area) {
        return None;
    }
    let directory = root.join(area);
    if !real_directory(&directory) || !contained(root, &directory) {
        return None;
    }
    let mut paths = Vec::new();
    collect_sources(&directory, 0, &mut paths);
    let mut result = String::new();
    let mut used = 0;
    for path in paths.into_iter().take(MAX_FILES_PER_AREA) {
        if used >= MAX_AREA_CHARS || !contained(root, &path) || !regular_file(&path) {
            continue;
        }
        let Ok(file) = fs::File::open(&path) else {
            continue;
        };
        let mut bytes = Vec::new();
        if file
            .take((MAX_FILE_CHARS * 4) as u64)
            .read_to_end(&mut bytes)
            .is_err()
        {
            continue;
        }
        let content = String::from_utf8_lossy(&bytes);
        let relative = path
            .strip_prefix(root)
            .ok()?
            .to_string_lossy()
            .replace('\\', "/");
        let room = (MAX_AREA_CHARS - used).min(MAX_FILE_CHARS);
        let excerpt = crate::core::context_build::clip(&content, room);
        used += excerpt.chars().count();
        result.push_str(&format!("\n--- repo:{relative} ---\n{excerpt}\n"));
    }
    (!result.is_empty()).then_some(result)
}

fn list_source_names(directory: &Path, depth: usize, cap: usize) -> Vec<String> {
    if depth > 2 || !real_directory(directory) {
        return Vec::new();
    }
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut entries = entries.flatten().collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());
    let mut names = Vec::new();
    for entry in entries {
        if names.len() >= cap {
            break;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || SKIP_DIRS.contains(&name.as_str()) {
            continue;
        }
        let path = entry.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_file() && source_file(&path) {
            names.push(name);
        } else if meta.is_dir() {
            for child in list_source_names(&path, depth + 1, cap - names.len()) {
                names.push(format!("{name}/{child}"));
            }
        }
    }
    names
}

fn collect_sources(directory: &Path, depth: usize, out: &mut Vec<std::path::PathBuf>) {
    if depth > 3 || out.len() >= MAX_FILES_PER_AREA || !real_directory(directory) {
        return;
    }
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut entries = entries.flatten().collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        if out.len() >= MAX_FILES_PER_AREA {
            return;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || SKIP_DIRS.contains(&name.as_str()) {
            continue;
        }
        let path = entry.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_file() && source_file(&path) {
            out.push(path);
        } else if meta.is_dir() {
            collect_sources(&path, depth + 1, out);
        }
    }
}

fn source_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| SOURCE_EXTENSIONS.contains(&extension))
}

fn safe_relative(path: &str) -> bool {
    !path.is_empty()
        && Path::new(path)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

fn skipped(path: &str) -> bool {
    path.split('/')
        .any(|part| part.starts_with('.') || SKIP_DIRS.contains(&part))
}

fn contained(root: &Path, path: &Path) -> bool {
    root.canonicalize()
        .ok()
        .zip(path.canonicalize().ok())
        .is_some_and(|(root, path)| path.starts_with(root))
}

fn real_directory(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
}

fn regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
}
