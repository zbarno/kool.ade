use super::support::change;
use super::*;

#[test]
fn validator_accepts_only_koolade_markdown_and_rejects_unsafe_documents() {
    let valid = vec![change("docs/koolade/product/overview.md", "# Overview\n")];
    validate_changes(&[change("docs/koolade/features/AGENTS.md", "# Navigation\n")]).unwrap();
    validate_changes(&[change("docs/koolade/AGENTS.md", "# Navigation\n")]).unwrap();
    validate_changes(&valid).unwrap();
    for path in [
        "README.md",
        "docs/koolade/product/../../README.md",
        "docs/koolade/features/new\\feature.md",
        "docs/koolade/unapproved.md",
        "docs/koolade/features/nested/AGENTS.md",
        "docs/koolade/architecture/image.png",
    ] {
        assert!(
            validate_changes(&[change(path, "# no\n")]).is_err(),
            "{path}"
        );
    }
    assert!(validate_changes(&[change("docs/koolade/features/new.md", "\0")]).is_err());
    assert!(
        validate_changes(&[
            change("docs/koolade/features/new.md", "# A\n"),
            change("docs/koolade/features/new.md", "# B\n"),
        ])
        .is_err()
    );
    assert!(validate_changes(&[change("docs/koolade/product/empty.md", " ")]).is_err());
    assert!(
        validate_changes(&[change(
            "docs/koolade/product/large.md",
            &"x".repeat(MAX_DOCUMENT_BYTES + 1),
        )])
        .is_err()
    );
    assert!(
        validate_changes(
            &(0..=MAX_DOCUMENT_COUNT)
                .map(|index| change(
                    &format!("docs/koolade/features/feature-{index}.md"),
                    "# Feature\n"
                ))
                .collect::<Vec<_>>()
        )
        .is_err()
    );
}

#[test]
fn consent_is_bound_to_the_exact_document_paths_and_contents() {
    let approved = vec![change("docs/koolade/product/overview.md", "# Approved\n")];
    let consent = PublicationConsent::confirmed(&approved).unwrap();
    assert_eq!(consent.approved_document_digest, digest_changes(&approved));
    assert_ne!(
        consent.approved_document_digest,
        digest_changes(&[change("docs/koolade/product/overview.md", "# Changed\n")])
    );
}

#[test]
fn origin_identity_accepts_only_supported_public_github_remote_shapes() {
    assert_eq!(
        github_repository_from_remote("https://github.com/zbarno/kool.ade.git").unwrap(),
        Some("zbarno/kool.ade".into())
    );
    assert_eq!(
        github_repository_from_remote("git@github.com:zbarno/kool.ade.git").unwrap(),
        Some("zbarno/kool.ade".into())
    );
    assert_eq!(
        github_repository_from_remote("file:///tmp/example.git").unwrap(),
        None
    );
}
