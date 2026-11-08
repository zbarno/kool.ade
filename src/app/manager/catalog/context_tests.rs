use super::*;
use crate::harness::prompt_catalog::{EvidenceConfidentiality, EvidenceOrigin};
use std::process::Command;

fn item(id: &str) -> EvidenceItem {
    EvidenceItem {
        id: id.into(),
        workspace_uid: "workspace".into(),
        origin: EvidenceOrigin::Application,
        confidentiality: EvidenceConfidentiality::WorkspacePrivate,
        source: "fixture source".into(),
        content: "fixture evidence".into(),
    }
}

#[test]
fn manager_context_ranks_sources_by_authority_and_question_type() {
    let evidence = vec![
        item("workspace-summary"),
        item("generic-capability-inventory"),
        item("published-spec:abc"),
        item("authorized-task-direction:decision-1"),
        item("repository-architecture-profile"),
    ];
    let (selected, ranked) =
        resolve_sources(evidence, "How is repository architecture implemented?");
    assert!(ranked[0].contains("repository-architecture-profile"));
    assert!(
        ranked
            .iter()
            .any(|entry| entry.contains("published-spec:abc"))
    );
    assert!(
        ranked
            .iter()
            .any(|entry| entry.contains("authorized-task-direction:decision-1"))
    );
    assert!(selected.iter().any(|item| item.id == "workspace-summary"));
    assert!(
        ranked
            .iter()
            .any(|entry| entry.contains("workspace-summary"))
    );
}

#[test]
fn manager_context_records_omitted_source_count_under_the_cap() {
    let evidence = (0..14)
        .map(|index| item(&format!("evidence-{index}")))
        .collect::<Vec<_>>();
    let (selected, ranked) = resolve_sources(evidence, "status update");
    assert_eq!(selected.len(), 12);
    assert_eq!(ranked.len(), 13);
    assert!(ranked.last().unwrap().contains("omitted_sources=2"));
    assert!(ranked.last().unwrap().contains("omitted_ids="));
    assert_eq!(
        selected
            .iter()
            .map(|item| item.id.as_str())
            .collect::<std::collections::HashSet<_>>()
            .len(),
        12
    );
}

#[test]
fn manager_context_enforces_aggregate_evidence_byte_budget() {
    let mut large_a = item("evidence-large-a");
    large_a.content = "a".repeat(28 * 1024);
    let mut large_b = item("evidence-large-b");
    large_b.content = "b".repeat(28 * 1024);
    let (selected, selection) = resolve_sources(
        vec![item("workspace-summary"), large_a, large_b],
        "status update",
    );
    assert!(selected.iter().any(|item| item.id == "workspace-summary"));
    assert!(
        selected
            .iter()
            .map(|item| item.content.len())
            .sum::<usize>()
            < 48 * 1024
    );
    assert_eq!(selected.len(), 2);
    assert!(selection.last().unwrap().contains("omitted_sources=1"));
}

#[test]
fn manager_context_budget_accounts_for_json_escaping() {
    let mut escaped_a = item("evidence-escaped-a");
    escaped_a.content = "\\".repeat(20 * 1024);
    let mut escaped_b = item("evidence-escaped-b");
    escaped_b.content = "\\".repeat(20 * 1024);
    let (selected, selection) = resolve_sources(
        vec![item("workspace-summary"), escaped_a, escaped_b],
        "status update",
    );
    assert_eq!(selected.len(), 2);
    assert!(selected.iter().any(|item| item.id == "workspace-summary"));
    assert!(selection.last().unwrap().contains("omitted_sources=1"));
}

#[test]
fn manager_context_telemetry_tracks_selected_bytes_estimate_latency_and_omissions() {
    let evidence = vec![item("workspace-summary")];
    let telemetry = telemetry(
        &evidence,
        &[
            "rank=3 evidence_id=workspace-summary".into(),
            "omitted_sources=2 omitted_ids=source-a,source-b omitted_ids_truncated=false".into(),
        ],
        std::time::Duration::from_millis(17),
    );
    assert_eq!(telemetry.selected_source_ids, ["workspace-summary"]);
    assert_eq!(telemetry.omitted_source_count, 2);
    assert_eq!(telemetry.omitted_source_ids, "source-a,source-b");
    assert_eq!(telemetry.preparation_millis, 17);
    assert_eq!(telemetry.estimated_context_tokens, 4);
    assert_eq!(telemetry.selected_evidence_bytes, 16);
}

/// Production boundary: the Manager candidate router reads navigation only
/// from the configured immutable specification commit, then fetches a normal
/// document. Parser-only tests cannot prove that `AGENTS.md` bodies are never
/// returned as evidence. Uses temporary local Git only; no Docker/network;
/// setup cost is a few Git operations.
#[test]
fn published_navigation_routes_to_documents_without_selecting_agents_files() {
    let root = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| {
        let result = Command::new("git")
            .args(args)
            .current_dir(root.path())
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    };
    run(&["init", "-q", "-b", "main"]);
    run(&["config", "user.name", "Fixture User"]);
    run(&["config", "user.email", "fixture@example.invalid"]);
    let architecture = root.path().join("docs/koolade/architecture");
    std::fs::create_dir_all(&architecture).unwrap();
    std::fs::write(
        root.path().join("docs/koolade/AGENTS.md"),
        "# Navigation\n- [Architecture](./architecture/AGENTS.md) - architecture specifications\n",
    )
    .unwrap();
    std::fs::write(
        architecture.join("AGENTS.md"),
        "untrusted instruction text\n# Navigation\n- [Overview](./overview.md) - implementation architecture details\n",
    )
    .unwrap();
    std::fs::write(
        architecture.join("overview.md"),
        "Pinned architecture requirement.",
    )
    .unwrap();
    run(&["add", "."]);
    run(&["commit", "-qm", "fixture"]);

    let reader = crate::core::published_specs::PublishedSpecReader::new(
        root.path(),
        "fixture-primary",
        "refs/heads/main",
    )
    .unwrap();
    let pin = reader.pin().unwrap();
    let candidates = published_candidates(&reader, &pin, &terms("architecture implementation"));
    assert_eq!(
        candidates,
        vec![(2, "docs/koolade/architecture/overview.md".into())]
    );
    let selected = reader.read(&pin, &candidates[0].1).unwrap();
    assert_eq!(selected.content, "Pinned architecture requirement.");
}
