use std::path::Path;

pub(crate) fn is_manifest(path: &str) -> bool {
    let name = path
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(
        name.as_str(),
        "cargo.toml"
            | "package.json"
            | "package-lock.json"
            | "pnpm-lock.yaml"
            | "yarn.lock"
            | "pyproject.toml"
            | "requirements.txt"
            | "setup.py"
            | "setup.cfg"
            | "go.mod"
            | "go.sum"
            | "pom.xml"
            | "build.gradle"
            | "build.gradle.kts"
            | "settings.gradle"
            | "settings.gradle.kts"
            | "composer.json"
            | "gemfile"
            | "makefile"
            | "dockerfile"
            | "docker-compose.yml"
            | "docker-compose.yaml"
    ) || name.ends_with(".csproj")
        || name.ends_with(".sln")
}
pub(crate) fn is_doc(path: &str) -> bool {
    path.to_ascii_lowercase().ends_with(".md")
}
pub(crate) fn is_test(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let name = lower.rsplit('/').next().unwrap_or_default();
    lower.contains("/tests/")
        || lower.contains("/__tests__/")
        || name.starts_with("test_")
        || name.ends_with("_test.rs")
        || name.ends_with("_test.go")
        || name.ends_with(".test.ts")
        || name.ends_with(".test.tsx")
        || name.ends_with(".spec.ts")
        || name.ends_with(".spec.js")
        || name.ends_with("_test.py")
}
pub(crate) fn source_extension(path: &str) -> Option<&'static str> {
    let extension = Path::new(path).extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "rs" => Some("Rust"),
        "ts" | "tsx" => Some("TypeScript"),
        "js" | "jsx" | "mjs" | "cjs" => Some("JavaScript"),
        "py" => Some("Python"),
        "go" => Some("Go"),
        "java" => Some("Java"),
        "kt" | "kts" => Some("Kotlin"),
        "cs" => Some("C#"),
        "c" | "h" => Some("C"),
        "cc" | "cpp" | "cxx" | "hpp" => Some("C++"),
        "swift" => Some("Swift"),
        "php" => Some("PHP"),
        "rb" => Some("Ruby"),
        "ex" | "exs" => Some("Elixir"),
        "scala" => Some("Scala"),
        "dart" => Some("Dart"),
        _ => None,
    }
}
