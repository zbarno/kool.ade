#[test]
fn refreshed_product_index_preserves_heading_and_intro() {
    let temp = tempfile::tempdir().expect("temporary planning repository");
    let repository = temp.path();
    let store = crate::artifacts::planning_store::PlanningStore::new(
        uuid::Uuid::new_v4(),
        repository,
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    );
    let index_path = store.layout().product_index();
    std::fs::create_dir_all(index_path.parent().unwrap()).unwrap();
    std::fs::create_dir_all(store.layout().changes_root()).unwrap();
    let manifest = super::ProductManifest::core();
    for module in &manifest.modules {
        std::fs::write(
            store.layout().product_root().join(&module.path),
            format!("# {}\n", module.title),
        )
        .unwrap();
    }
    std::fs::write(
        store.layout().product_manifest(),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    let index =
        "# Kool.ad/e — Living Technical Specification\n\nFixture introduction.\n\n## Modules\n";
    std::fs::write(&index_path, index).unwrap();
    let refreshed =
        super::refreshed_index_from(repository, index, &[]).expect("refresh fixture product index");

    assert_eq!(
        index.lines().next(),
        Some("# Kool.ad/e — Living Technical Specification")
    );
    assert_eq!(refreshed.lines().next(), index.lines().next());
    assert_eq!(index_intro(&refreshed), index_intro(index));
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
