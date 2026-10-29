use std::{collections::VecDeque, path::Path};

use super::super::AppSurface;

mod path_support;

const MAX_DEPTH: usize = 6;
const MAX_ENTRIES: usize = 10_000;
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
const SKIP_DIRECTORIES: &[&str] = &[
    ".git",
    ".koolade-packet",
    "node_modules",
    "target",
    "vendor",
    "build",
    "dist",
    ".venv",
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveryEvidence {
    pub language: String,
    pub framework: Option<String>,
    pub evidence_path: String,
    pub supporting_evidence_paths: Vec<String>,
    pub candidate_surface: AppSurface,
    pub confidence: u8,
}

/// Inspects bounded project metadata only. It never invokes scripts or follows symlinks.
pub fn detect_candidate_targets(root: &Path) -> Result<Vec<DiscoveryEvidence>, String> {
    let canonical_root = root
        .canonicalize()
        .map_err(|_| "project root is unavailable".to_owned())?;
    if !canonical_root.is_dir() {
        return Err("project root is not a directory".into());
    }
    let mut pending = VecDeque::from([(canonical_root.clone(), 0_usize)]);
    let mut evidence = Vec::new();
    let mut visited = 0_usize;
    while let Some((directory, depth)) = pending.pop_front() {
        let entries = std::fs::read_dir(&directory)
            .map_err(|_| "project metadata directory could not be read".to_owned())?;
        for entry in entries {
            visited += 1;
            if visited > MAX_ENTRIES {
                return Err("project metadata scan exceeded its directory-entry limit".into());
            }
            let entry = entry.map_err(|_| "project metadata entry could not be read".to_owned())?;
            let file_type = entry
                .file_type()
                .map_err(|_| "project metadata entry type could not be read".to_owned())?;
            if file_type.is_symlink() {
                continue;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if file_type.is_dir() {
                if depth < MAX_DEPTH && !SKIP_DIRECTORIES.contains(&name.as_ref()) {
                    pending.push_back((entry.path(), depth + 1));
                }
                continue;
            }
            if !file_type.is_file() {
                continue;
            }
            let metadata = entry
                .metadata()
                .map_err(|_| "project metadata file could not be inspected".to_owned())?;
            if metadata.len() > MAX_CONFIG_BYTES {
                continue;
            }
            if let Some((language, framework, surface, confidence)) = classify(&name, &entry.path())
            {
                let relative = entry
                    .path()
                    .strip_prefix(&canonical_root)
                    .map_err(|_| "project metadata escaped its root".to_owned())?
                    .to_string_lossy()
                    .replace('\\', "/");
                let supporting_evidence_paths = if name == "package.json"
                    && surface == AppSurface::Web
                    && entry.path().parent().is_some_and(|root| {
                        path_support::has_regular_relative_file(root, &["src", "index.html"])
                    }) {
                    let target_root = Path::new(&relative)
                        .parent()
                        .filter(|parent| !parent.as_os_str().is_empty())
                        .map(ToOwned::to_owned)
                        .unwrap_or_default();
                    vec![
                        target_root
                            .join("src/index.html")
                            .to_string_lossy()
                            .replace('\\', "/"),
                    ]
                } else {
                    Vec::new()
                };
                evidence.push(DiscoveryEvidence {
                    language: language.into(),
                    framework: framework.map(str::to_owned),
                    evidence_path: relative,
                    supporting_evidence_paths,
                    candidate_surface: surface,
                    confidence,
                });
            }
        }
    }
    evidence.sort_by(|left, right| left.evidence_path.cmp(&right.evidence_path));
    Ok(evidence)
}

fn classify(
    name: &str,
    path: &Path,
) -> Option<(&'static str, Option<&'static str>, AppSurface, u8)> {
    match name {
        "Cargo.toml" => {
            let text = read_config(path)?;
            if ["eframe", "tauri", "winit", "iced", "slint"]
                .iter()
                .any(|marker| text.contains(marker))
            {
                Some((
                    "Rust",
                    Some("Rust desktop candidate"),
                    AppSurface::LinuxDesktop,
                    75,
                ))
            } else {
                Some(("Rust", Some("Cargo"), AppSurface::HeadlessService, 55))
            }
        }
        "package.json" => {
            let text = read_config(path)?;
            let framework_marker = ["next", "vite", "react", "vue", "angular", "svelte"]
                .iter()
                .any(|marker| text.contains(marker));
            let html_entrypoint = path.parent().is_some_and(|root| {
                path_support::has_regular_relative_file(root, &["src", "index.html"])
            });
            if framework_marker || html_entrypoint {
                Some((
                    "JavaScript",
                    Some("Node.js web candidate"),
                    AppSurface::Web,
                    75,
                ))
            } else {
                Some((
                    "JavaScript",
                    Some("Node.js"),
                    AppSurface::HeadlessService,
                    45,
                ))
            }
        }
        "pyproject.toml" | "requirements.txt" => {
            let text = read_config(path)?.to_ascii_lowercase();
            if ["fastapi", "flask", "django", "starlette"]
                .iter()
                .any(|marker| text.contains(marker))
            {
                Some(("Python", Some("Python web candidate"), AppSurface::Web, 75))
            } else if ["pyside", "pyqt", "kivy"]
                .iter()
                .any(|marker| text.contains(marker))
            {
                Some((
                    "Python",
                    Some("Python desktop candidate"),
                    AppSurface::LinuxDesktop,
                    70,
                ))
            } else {
                Some(("Python", Some("Python"), AppSurface::HeadlessService, 45))
            }
        }
        "go.mod" => {
            let text = read_config(path)?.to_ascii_lowercase();
            let (framework, surface, confidence) = if [
                "gin-gonic/gin",
                "labstack/echo",
                "gofiber/fiber",
                "go-chi/chi",
            ]
            .iter()
            .any(|marker| text.contains(marker))
            {
                ("Go web candidate", AppSurface::Web, 70)
            } else {
                ("Go modules", AppSurface::HeadlessService, 55)
            };
            Some(("Go", Some(framework), surface, confidence))
        }
        "pom.xml" => {
            let text = read_config(path)?.to_ascii_lowercase();
            let (framework, surface, confidence) = if text.contains("javafx") {
                ("Java desktop candidate", AppSurface::LinuxDesktop, 65)
            } else if ["spring-boot", "quarkus", "jakarta.ws.rs"]
                .iter()
                .any(|marker| text.contains(marker))
            {
                ("Java web candidate", AppSurface::Web, 70)
            } else {
                ("Maven", AppSurface::HeadlessService, 55)
            };
            Some(("Java", Some(framework), surface, confidence))
        }
        "build.gradle" | "build.gradle.kts" => {
            let text = read_config(path)?.to_ascii_lowercase();
            let (framework, surface, confidence) = if text.contains("javafx") {
                ("Java desktop candidate", AppSurface::LinuxDesktop, 65)
            } else if ["spring-boot", "quarkus", "ktor", "http4k"]
                .iter()
                .any(|marker| text.contains(marker))
            {
                ("Java/Kotlin web candidate", AppSurface::Web, 65)
            } else {
                ("Gradle", AppSurface::HeadlessService, 50)
            };
            Some(("Java/Kotlin", Some(framework), surface, confidence))
        }
        "CMakeLists.txt" => {
            let text = read_config(path)?.to_ascii_lowercase();
            let surface = if ["qt", "gtk", "sdl", "wxwidgets"]
                .iter()
                .any(|marker| text.contains(marker))
            {
                AppSurface::LinuxDesktop
            } else {
                AppSurface::UnsupportedTarget
            };
            Some(("C/C++", Some("CMake"), surface, 45))
        }
        "pubspec.yaml" => {
            let parent = path.parent()?;
            let surface = if parent.join("linux").is_dir() {
                AppSurface::LinuxDesktop
            } else if parent.join("web").is_dir() {
                AppSurface::Web
            } else {
                AppSurface::UnsupportedTarget
            };
            let confidence = if surface == AppSurface::UnsupportedTarget {
                35
            } else {
                65
            };
            Some(("Dart", Some("Flutter"), surface, confidence))
        }
        "Makefile" => Some(("Unknown", Some("Make"), AppSurface::UnsupportedTarget, 25)),
        "compose.yml" | "compose.yaml" | "docker-compose.yml" | "docker-compose.yaml" => Some((
            "Unknown",
            Some("Compose metadata (never executed implicitly)"),
            AppSurface::UnsupportedTarget,
            10,
        )),
        _ if name.ends_with(".csproj") => {
            let text = read_config(path)?.to_ascii_lowercase();
            let surface = if text.contains("avalonia") {
                AppSurface::LinuxDesktop
            } else if ["usewpf", "usewindowsforms"]
                .iter()
                .any(|marker| text.contains(marker))
            {
                AppSurface::UnsupportedTarget
            } else {
                AppSurface::HeadlessService
            };
            Some((".NET", Some(".NET project"), surface, 55))
        }
        _ if name.ends_with(".sln") => Some((
            ".NET",
            Some("Solution; select an application project explicitly"),
            AppSurface::UnsupportedTarget,
            35,
        )),
        _ => None,
    }
}

fn read_config(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}
