use std::path::Path;

#[test]
fn current_product_index_heading_and_intro_survive_app_refresh() {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"));
    let index_path = crate::artifacts::layout::ArtifactLayout::new(repository).product_index();
    let index = std::fs::read_to_string(index_path).expect("read checked-in product index");
    let refreshed = super::refreshed_index_from(repository, &index, &[])
        .expect("refresh checked-in product index");

    assert_eq!(
        index.lines().next(),
        Some("# Kool.ad/e — Living Technical Specification")
    );
    assert_eq!(refreshed.lines().next(), index.lines().next());
    assert_eq!(index_intro(&refreshed), index_intro(&index));
}

fn index_intro(index: &str) -> String {
    index
        .lines()
        .skip(1)
        .take_while(|line| {
            !line.starts_with("## Modules")
                && !line.starts_with("## Product modules")
                && !line.starts_with("## Active features")
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}
