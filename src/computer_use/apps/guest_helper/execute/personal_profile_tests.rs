use super::launch_spec;

#[test]
fn browser_authentication_profile_is_stored_on_the_guest_disk() {
    for app in ["chromium", "chromium-research"] {
        let (_, args) = launch_spec(app).unwrap();
        assert!(
            args.iter()
                .any(|arg| { *arg == "--user-data-dir=/home/koolade/.cache/koolade-chromium" })
        );
        assert!(
            args.iter().any(|arg| {
                *arg == "--disk-cache-dir=/home/koolade/.cache/koolade-chromium-cache"
            })
        );
        assert!(!args.iter().any(|arg| arg.contains("/run/")));
    }
}
