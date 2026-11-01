use sha2::Digest;

use crate::app::session::Project;

const MAX_ITEMS: usize = 256;
const MAX_REFERENCE_BYTES: usize = 4096;
const MAX_FINDING_BYTES: usize = 4096;

pub(super) fn event_cursor(events: &[String]) -> String {
    let mut digest = sha2::Sha256::new();
    digest.update(b"koolade-manager-event-batch-v1\0");
    digest.update((events.len() as u64).to_be_bytes());
    for event in events {
        digest.update((event.len() as u64).to_be_bytes());
        digest.update(event.as_bytes());
    }
    format!("event-batch:{:x}", digest.finalize())
}

pub(super) fn work_graph(project: &Project) -> Vec<String> {
    bounded_references(
        project.active_implementations.keys().cloned(),
        MAX_REFERENCE_BYTES,
    )
}

pub(super) fn unresolved_findings(project: &Project) -> Vec<String> {
    project
        .queue
        .blocked
        .iter()
        .take(MAX_ITEMS)
        .map(|(task, issue)| {
            let description = format!("{task}: {}", issue.message).replace('\0', "");
            crate::core::context_build::clip_bytes(&description, MAX_FINDING_BYTES)
        })
        .filter(|finding| valid_text(finding, MAX_FINDING_BYTES))
        .collect()
}

pub(super) fn latest_review_receipts(receipts: Vec<String>) -> Vec<String> {
    bounded_references(receipts, 512)
}

fn bounded_references(values: impl IntoIterator<Item = String>, max_bytes: usize) -> Vec<String> {
    let mut references = values
        .into_iter()
        .filter(|value| valid_text(value, max_bytes))
        .collect::<Vec<_>>();
    references.sort();
    references.truncate(MAX_ITEMS);
    references
}

fn valid_text(value: &str, max_bytes: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max_bytes && !value.contains('\0')
}

#[cfg(test)]
mod tests {
    use super::event_cursor;

    #[test]
    fn event_cursor_is_bounded_for_empty_and_long_batches() {
        let empty = event_cursor(&[]);
        let long = event_cursor(&["é".repeat(1600)]);

        assert!(empty.len() <= 128);
        assert!(long.len() <= 128);
        assert_ne!(empty, long);
        assert_eq!(empty, event_cursor(&[]));
    }
}
