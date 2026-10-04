use super::super::ChatTabs;
#[test]
fn multiple_tabs_keep_open_order_and_close_only_the_selected_conversation() {
    let mut tabs = ChatTabs::default();
    tabs.open("first");
    tabs.open("second");
    tabs.open("first");
    assert_eq!(tabs.keys, ["first", "second"]);
    assert_eq!(tabs.active.as_deref(), Some("first"));
    tabs.close("second");
    assert_eq!(tabs.active.as_deref(), Some("first"));
    tabs.open("third");
    tabs.close("third");
    assert!(tabs.active.is_none());
    assert_eq!(tabs.keys, ["first"]);
}
