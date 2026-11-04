use super::DocumentationChange;
use crate::artifacts::product_docs::CoreConcept;

/// Build the reviewed starter layout for a repository with no published Kool.ad/e
/// product modules. The caller still has to present it and obtain explicit consent.
pub fn initial_product_documents(title: &str) -> Vec<DocumentationChange> {
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let product = [
        (
            CoreConcept::Overview,
            format!("The purpose and intended value of {title} have not yet been confirmed. Describe the project in the Specification conversation."),
        ),
        (
            CoreConcept::UsersAndOutcomes,
            "Target users, their needs, desired outcomes, and ownership require confirmation.".to_owned(),
        ),
        (
            CoreConcept::CurrentCapabilities,
            "Current behavior and supported capabilities require repository evidence and confirmation. Planned work is not current capability.".to_owned(),
        ),
        (
            CoreConcept::ArchitectureAndConstraints,
            "Inspect the repository and deployment context before recording architecture, data boundaries, or runtime constraints.".to_owned(),
        ),
        (
            CoreConcept::Decisions,
            "| ID | Decision | Basis | Status |\n| --- | --- | --- | --- |".to_owned(),
        ),
        (
            CoreConcept::QualityAndAcceptance,
            "Quality expectations, risks, and acceptance evidence have not yet been established.".to_owned(),
        ),
    ];
    let mut changes = product
        .iter()
        .map(|(concept, content)| DocumentationChange {
            path: format!("docs/koolade/product/{}.md", concept.id()),
            content: format!("# {}\n\n{}\n", concept.title(), content),
        })
        .collect::<Vec<_>>();
    let module_links = product
        .iter()
        .map(|(concept, _)| format!("- [{}](./{}.md)", concept.title(), concept.id()))
        .collect::<Vec<_>>()
        .join("\n");
    changes.extend([
        DocumentationChange {
            path: "docs/koolade/AGENTS.md".into(),
            content: "# Navigation\n\n- [product/](./product/AGENTS.md) - Published product purpose, capabilities, and constraints.\n- [features/](./features/AGENTS.md) - Published feature specifications.\n- [architecture/](./architecture/AGENTS.md) - Published architecture decisions.\n".into(),
        },
        DocumentationChange {
            path: "docs/koolade/product/AGENTS.md".into(),
            content: format!("# Navigation\n\n{module_links}\n- [index.md](./index.md) - Product specification module list.\n"),
        },
        DocumentationChange {
            path: "docs/koolade/product/index.md".into(),
            content: format!("# {title}\n\nStarter product specification. Unknowns are labeled and require evidence or user confirmation.\n\n## Product modules\n\n{module_links}\n"),
        },
        DocumentationChange {
            path: "docs/koolade/features/AGENTS.md".into(),
            content: "# Navigation\n\n- [index.md](./index.md) - Feature specification publication status.\n".into(),
        },
        DocumentationChange {
            path: "docs/koolade/features/index.md".into(),
            content: "# Published Features\n\nNo feature specifications have been published yet.\n".into(),
        },
        DocumentationChange {
            path: "docs/koolade/architecture/AGENTS.md".into(),
            content: "# Navigation\n\n- [index.md](./index.md) - Architecture decision publication status.\n".into(),
        },
        DocumentationChange {
            path: "docs/koolade/architecture/index.md".into(),
            content: "# Architecture Decisions\n\nNo architecture decisions have been published yet.\n".into(),
        },
    ]);
    changes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_layout_is_complete_validated_and_indexed() {
        let changes = initial_product_documents("Demo Project");
        super::super::validate_changes(&changes).unwrap();
        assert_eq!(changes.len(), 13);
        let product_index = changes
            .iter()
            .find(|change| change.path == "docs/koolade/product/AGENTS.md")
            .unwrap();
        for concept in CoreConcept::all() {
            assert!(
                product_index
                    .content
                    .contains(&format!("./{}.md", concept.id()))
            );
        }
        assert!(changes.iter().all(|change| {
            !change.content.contains("planning/tasks")
                && !change.content.contains(".koolade-packet")
        }));
    }
}
