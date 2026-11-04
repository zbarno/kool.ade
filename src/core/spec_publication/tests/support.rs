use super::*;

pub(super) fn change(path: &str, content: &str) -> DocumentationChange {
    DocumentationChange {
        path: path.into(),
        content: content.into(),
    }
}
