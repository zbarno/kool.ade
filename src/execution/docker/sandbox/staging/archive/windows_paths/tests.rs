use super::{validate_collisions, validate_path};
use std::path::Path;

#[test]
fn rejects_unsupported_windows_path_forms_and_components() {
    let too_long = format!("folder/{}", "x".repeat(256));
    for path in [
        r"C:\workspace\source.txt",
        r"\server\share\source.txt",
        "source.txt:secret",
        "CON",
        "nul.txt",
        "COM1.log",
        "folder/trailing.",
        "folder/trailing ",
        "folder/bad?name",
        too_long.as_str(),
    ] {
        assert!(validate_path(Path::new(path)).is_err(), "accepted {path:?}");
    }
}

#[test]
fn rejects_short_name_alias_shaped_components() {
    for path in ["LONGFI~1.TXT", "nested/LONGDI~1/LONGFI~1.TXT", "a~10.md"] {
        assert!(validate_path(Path::new(path)).is_err(), "accepted {path:?}");
    }

    for path in ["release~2026.md", "component~1preview.txt"] {
        assert!(validate_path(Path::new(path)).is_ok(), "rejected {path:?}");
    }

    let long_name_and_alias = [Path::new("Long File.txt"), Path::new("LONGFI~1.TXT")];
    assert!(validate_collisions(long_name_and_alias.into_iter()).is_err());
}

#[test]
fn rejects_case_collisions_in_any_directory_component() {
    for paths in [
        vec![Path::new("Readme.txt"), Path::new("README.txt")],
        vec![Path::new("Build/a.rs"), Path::new("build/b.rs")],
    ] {
        assert!(validate_collisions(paths.into_iter()).is_err());
    }
    let separator_aliases = [Path::new("nested/file.txt"), Path::new(r"nested\file.txt")];
    assert!(validate_collisions(separator_aliases.into_iter()).is_err());
}

#[cfg(windows)]
#[test]
fn unicode_collisions_follow_windows_ordinal_case_rules() {
    let greek_case_pair = [Path::new("Σ.txt"), Path::new("ς.TXT")];
    assert!(validate_collisions(greek_case_pair.into_iter()).is_err());
    assert!(super::windows_ordinal_eq_ignore_case("Σ.txt", "ς.TXT").unwrap());

    let sharp_s_pair = [Path::new("ß.txt"), Path::new("SS.TXT")];
    let sharp_s_equal = super::windows_ordinal_compare(
        &"ß.txt".encode_utf16().collect::<Vec<_>>(),
        &"SS.TXT".encode_utf16().collect::<Vec<_>>(),
    ) == Some(std::cmp::Ordering::Equal);
    assert_eq!(
        super::windows_ordinal_eq_ignore_case("ß.txt", "SS.TXT").unwrap(),
        sharp_s_equal
    );
    assert_eq!(
        validate_collisions(sharp_s_pair.into_iter()).is_err(),
        sharp_s_equal
    );
}

#[test]
fn accepts_distinct_windows_safe_unicode_paths() {
    let paths = [Path::new("src/naïve.rs"), Path::new("docs/東京.md")];
    assert!(validate_collisions(paths.into_iter()).is_ok());
}

#[cfg(not(windows))]
#[test]
fn rejects_unicode_normalization_collisions() {
    let paths = [Path::new("café.txt"), Path::new("cafe\u{301}.txt")];
    assert!(super::validate_case_collisions(paths.into_iter()).is_err());

    let casefolded = [Path::new("ß.txt"), Path::new("ẞ.TXT")];
    assert!(super::validate_case_collisions(casefolded.into_iter()).is_err());
}
