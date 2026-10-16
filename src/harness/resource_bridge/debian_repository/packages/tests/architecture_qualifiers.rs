use super::*;

#[test]
fn concrete_architecture_qualifiers_parse_and_match_only_the_target_architecture() {
    let app = record("app", Some("liba:amd64"));
    let index = PackageIndex {
        records: BTreeMap::from([("app".into(), app), ("liba".into(), record("liba", None))]),
    };
    let closure = index
        .closure(&["app".into()], &BTreeMap::new(), "amd64")
        .unwrap();
    assert_eq!(
        closure
            .iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>(),
        ["app", "liba"]
    );

    let app = record("app", Some("liba:armel"));
    let index = PackageIndex {
        records: BTreeMap::from([("app".into(), app), ("liba".into(), record("liba", None))]),
    };
    assert!(
        index
            .closure(&["app".into()], &BTreeMap::new(), "amd64")
            .is_err()
    );

    // This signed Bookworm index stanza is unrelated to amd64 dependency
    // closure, but full-index parsing must still admit its concrete qualifier.
    let crossbuild = record("crossbuild-essential-armel", Some("gcc:armel"));
    assert_eq!(crossbuild.dependencies.len(), 1);
}
