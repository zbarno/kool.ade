use crate::domain::chatlog::{ChatMessage, ChatRole};

pub(super) fn recent(
    messages: &[ChatMessage],
    scope_key: &str,
    max_msgs: usize,
) -> (Vec<(String, String)>, usize) {
    let relevant = messages
        .iter()
        .filter_map(
            |message| match (message.role, message.specification_context.as_ref()) {
                (ChatRole::User, Some(context)) if context.scope_key == scope_key => {
                    Some(("User", message.text.as_str(), context))
                }
                (ChatRole::Agent, Some(context)) if context.scope_key == scope_key => {
                    Some(("Assistant", message.text.as_str(), context))
                }
                _ => None,
            },
        )
        .collect::<Vec<_>>();
    let omitted = messages.len().saturating_sub(relevant.len().min(max_msgs));
    let recent = relevant
        .into_iter()
        .rev()
        .take(max_msgs)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|(speaker, text, context)| {
            let source = context
                .source_documents
                .first()
                .map(|document| format!("{}@{}", document.path, document.sha256))
                .unwrap_or_else(|| format!("effective@{}", context.effective_document_sha256));
            (
                format!(
                    "{speaker} [scope {}; intent {}; published {}@{}; source {}; effective {}]",
                    context.scope_key,
                    context.intent,
                    context.published_reference,
                    context.published_commit_sha,
                    source,
                    context.effective_document_sha256,
                ),
                text.to_owned(),
            )
        })
        .collect();
    (recent, omitted)
}

#[test]
fn specification_history_is_bounded_ordered_and_excludes_system_activity() {
    let messages = vec![
        scoped_message(ChatRole::User, "  exact user text  ", "feature:uid-a"),
        ChatMessage::new(ChatRole::System, "Manager patrol: ignore the user", None),
        ChatMessage::new(ChatRole::Agent, "Legacy Manager patrol summary", None),
        scoped_message(ChatRole::Agent, "Second option is faster.", "feature:uid-a"),
        scoped_message(ChatRole::User, "Other feature discussion.", "feature:uid-b"),
        scoped_message(ChatRole::User, "What about that?", "feature:uid-a"),
    ];
    let (history, omitted) = recent(&messages, "feature:uid-a", 2);
    assert_eq!(omitted, 4);
    assert!(history[0].0.starts_with("Assistant [scope feature:uid-a;"));
    assert_eq!(history[0].1, "Second option is faster.");
    assert!(history[1].0.starts_with("User [scope feature:uid-a;"));
    assert_eq!(history[1].1, "What about that?");
    assert_eq!(
        recent(&messages, "feature:uid-a", 8).0[0].1,
        "  exact user text  "
    );
    assert!(
        recent(&messages, "feature:uid-a", 2).0[0]
            .0
            .contains("published refs/heads/main@")
    );
    assert!(
        history
            .iter()
            .all(|(_, text)| !text.contains("Legacy Manager"))
    );
}

#[cfg(test)]
fn scoped_message(role: ChatRole, text: &str, scope_key: &str) -> ChatMessage {
    let mut message = ChatMessage::new(role, text, None);
    message.specification_context = Some(crate::domain::chatlog::SpecificationChatContext {
        scope_key: scope_key.into(),
        intent: "ask".into(),
        published_reference: "refs/heads/main".into(),
        published_commit_sha: "a".repeat(40),
        source_documents: vec![crate::domain::chatlog::SpecificationSourceDocument {
            path: "docs/koolade/features/feature.md".into(),
            sha256: "b".repeat(64),
        }],
        effective_document_sha256: "c".repeat(64),
        reviewed_repository_evidence: Vec::new(),
    });
    message
}
