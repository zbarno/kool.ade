use super::*;

#[test]
fn clipboard_is_denied_by_default_expiring_visible_and_one_use_without_persistence() {
    let (_temp, bridge) = bridge();
    let identity = identity();
    assert!(
        bridge
            .grant_clipboard(
                identity.clone(),
                ComputerClipboardDirectionV1::CopyToGuest,
                NOW,
                NOW + 1000,
                3,
                1,
                false
            )
            .is_err()
    );
    let grant = bridge
        .grant_clipboard(
            identity.clone(),
            ComputerClipboardDirectionV1::PasteFromGuest,
            NOW,
            NOW + 1000,
            3,
            1,
            true,
        )
        .unwrap();
    let secret = b"sensitive clipboard text";
    assert!(
        bridge
            .consume_clipboard(
                &grant.grant_id,
                &identity,
                ComputerClipboardDirectionV1::PasteFromGuest,
                secret,
                4,
                1,
                NOW + 1
            )
            .is_err()
    );
    assert!(
        bridge
            .consume_clipboard(
                &grant.grant_id,
                &identity,
                ComputerClipboardDirectionV1::PasteFromGuest,
                secret,
                3,
                2,
                NOW + 1
            )
            .is_err()
    );
    bridge
        .consume_clipboard(
            &grant.grant_id,
            &identity,
            ComputerClipboardDirectionV1::PasteFromGuest,
            secret,
            3,
            1,
            NOW + 1,
        )
        .unwrap();
    assert!(
        bridge
            .consume_clipboard(
                &grant.grant_id,
                &identity,
                ComputerClipboardDirectionV1::PasteFromGuest,
                secret,
                3,
                1,
                NOW + 2
            )
            .is_err()
    );
    assert!(
        bridge
            .consume_clipboard(
                &grant.grant_id,
                &identity,
                ComputerClipboardDirectionV1::PasteFromGuest,
                secret,
                3,
                1,
                NOW + 1001
            )
            .is_err()
    );
    let files = bridge
        .store
        .list_files("computer-use/transfers/v1/clipboard")
        .unwrap();
    assert!(files.iter().all(|file| {
        !String::from_utf8_lossy(
            &bridge
                .store
                .read(format!("computer-use/transfers/v1/clipboard/{}", file.name))
                .unwrap(),
        )
        .contains("sensitive clipboard text")
    }));
}
