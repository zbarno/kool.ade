use crate::computer_use::{KeyCodeV1, MouseButtonV1, guest_protocol::GuestProtocolErrorV1};
use x11rb::{
    connection::Connection,
    protocol::{
        xproto::{self, ConnectionExt as _},
        xtest::ConnectionExt as _,
    },
};

pub(in crate::computer_use::guest_protocol::agent) fn click(
    x: u32,
    y: u32,
    button: MouseButtonV1,
) -> Result<(), GuestProtocolErrorV1> {
    let (connection, screen_index) = connect()?;
    let screen = &connection.setup().roots[screen_index];
    let geometry = connection
        .get_geometry(screen.root)
        .map_err(|_| GuestProtocolErrorV1::Io)?
        .reply()
        .map_err(|_| GuestProtocolErrorV1::Io)?;
    if x >= geometry.width as u32 || y >= geometry.height as u32 {
        return Err(GuestProtocolErrorV1::Malformed);
    }
    let button_code = match button {
        MouseButtonV1::Primary => 1,
        MouseButtonV1::Middle => 2,
        MouseButtonV1::Secondary => 3,
    };
    connection
        .xtest_fake_input(
            xproto::MOTION_NOTIFY_EVENT,
            0,
            xproto::Time::CURRENT_TIME.into(),
            screen.root,
            x as i16,
            y as i16,
            0,
        )
        .map_err(|_| GuestProtocolErrorV1::Io)?
        .check()
        .map_err(|_| GuestProtocolErrorV1::Io)?;
    for event in [xproto::BUTTON_PRESS_EVENT, xproto::BUTTON_RELEASE_EVENT] {
        connection
            .xtest_fake_input(
                event,
                button_code,
                xproto::Time::CURRENT_TIME.into(),
                screen.root,
                0,
                0,
                0,
            )
            .map_err(|_| GuestProtocolErrorV1::Io)?
            .check()
            .map_err(|_| GuestProtocolErrorV1::Io)?;
    }
    Ok(())
}

pub(in crate::computer_use::guest_protocol::agent) fn key_press(
    key: KeyCodeV1,
) -> Result<(), GuestProtocolErrorV1> {
    let keysym = match key {
        KeyCodeV1::Enter => 0xff0d,
        KeyCodeV1::Escape => 0xff1b,
        KeyCodeV1::Tab => 0xff09,
        KeyCodeV1::Backspace => 0xff08,
        KeyCodeV1::Delete => 0xffff,
        KeyCodeV1::ArrowUp => 0xff52,
        KeyCodeV1::ArrowDown => 0xff54,
        KeyCodeV1::ArrowLeft => 0xff51,
        KeyCodeV1::ArrowRight => 0xff53,
        KeyCodeV1::Home => 0xff50,
        KeyCodeV1::End => 0xff57,
        KeyCodeV1::PageUp => 0xff55,
        KeyCodeV1::PageDown => 0xff56,
        KeyCodeV1::Space => 0x20,
        KeyCodeV1::Control => 0xffe3,
        KeyCodeV1::Shift => 0xffe1,
        KeyCodeV1::Alt => 0xffe9,
        KeyCodeV1::Super => 0xffeb,
    };
    let (connection, screen_index) = connect()?;
    let screen = &connection.setup().roots[screen_index];
    let keycode = keycode_for_keysym(&connection, keysym, false)?;
    send_key(&connection, screen.root, keycode)
}

pub(in crate::computer_use::guest_protocol::agent) fn type_ascii(
    text: &str,
) -> Result<(), GuestProtocolErrorV1> {
    let (connection, screen_index) = connect()?;
    let root = connection.setup().roots[screen_index].root;
    let keymap = keyboard_map(&connection)?;
    let shift = keycode_for_keysym(&connection, 0xffe1, false)?;
    let keycodes = text
        .chars()
        .map(|character| {
            let (keysym, needs_shift) =
                ascii_keysym(character).ok_or(GuestProtocolErrorV1::UnsupportedCapability)?;
            Ok((
                keycode_for_keysym_from_map(&keymap, keysym, needs_shift)?,
                needs_shift,
            ))
        })
        .collect::<Result<Vec<_>, GuestProtocolErrorV1>>()?;
    for (keycode, needs_shift) in keycodes {
        if needs_shift {
            send_key(&connection, root, shift)?;
        }
        send_key(&connection, root, keycode)?;
        if needs_shift {
            send_key_release(&connection, root, shift)?;
        }
    }
    Ok(())
}

pub(super) fn connect() -> Result<(impl Connection, usize), GuestProtocolErrorV1> {
    x11rb::connect(Some(":0")).map_err(|_| GuestProtocolErrorV1::UnsupportedCapability)
}

fn keyboard_map<C: Connection>(
    connection: &C,
) -> Result<xproto::GetKeyboardMappingReply, GuestProtocolErrorV1> {
    let setup = connection.setup();
    connection
        .get_keyboard_mapping(
            setup.min_keycode,
            setup
                .max_keycode
                .saturating_sub(setup.min_keycode)
                .saturating_add(1),
        )
        .map_err(|_| GuestProtocolErrorV1::Io)?
        .reply()
        .map_err(|_| GuestProtocolErrorV1::Io)
}

fn keycode_for_keysym<C: Connection>(
    connection: &C,
    keysym: u32,
    shifted: bool,
) -> Result<u8, GuestProtocolErrorV1> {
    let map = keyboard_map(connection)?;
    keycode_for_keysym_from_map(&map, keysym, shifted)
}

fn keycode_for_keysym_from_map(
    map: &xproto::GetKeyboardMappingReply,
    keysym: u32,
    shifted: bool,
) -> Result<u8, GuestProtocolErrorV1> {
    let per_key = map.keysyms_per_keycode as usize;
    for (index, keysyms) in map.keysyms.chunks_exact(per_key).enumerate() {
        let column = if shifted { 1 } else { 0 };
        if keysyms.get(column).copied() == Some(keysym) {
            return Ok((index as u8).saturating_add(8));
        }
    }
    Err(GuestProtocolErrorV1::UnsupportedCapability)
}

fn ascii_keysym(character: char) -> Option<(u32, bool)> {
    let byte = u8::try_from(character as u32).ok()?;
    Some(match byte {
        b'a'..=b'z' | b'0'..=b'9' | b' ' => (byte as u32, false),
        b'A'..=b'Z' => ((byte + 32) as u32, true),
        b'!' => (b'1' as u32, true),
        b'@' => (b'2' as u32, true),
        b'#' => (b'3' as u32, true),
        b'$' => (b'4' as u32, true),
        b'%' => (b'5' as u32, true),
        b'^' => (b'6' as u32, true),
        b'&' => (b'7' as u32, true),
        b'*' => (b'8' as u32, true),
        b'(' => (b'9' as u32, true),
        b')' => (b'0' as u32, true),
        b'-' => (b'-' as u32, false),
        b'_' => (b'-' as u32, true),
        b'=' => (b'=' as u32, false),
        b'+' => (b'=' as u32, true),
        b'[' => (b'[' as u32, false),
        b'{' => (b'[' as u32, true),
        b']' => (b']' as u32, false),
        b'}' => (b']' as u32, true),
        b'\\' => (b'\\' as u32, false),
        b'|' => (b'\\' as u32, true),
        b';' => (b';' as u32, false),
        b':' => (b';' as u32, true),
        b'\'' => (b'\'' as u32, false),
        b'"' => (b'\'' as u32, true),
        b',' => (b',' as u32, false),
        b'<' => (b',' as u32, true),
        b'.' => (b'.' as u32, false),
        b'>' => (b'.' as u32, true),
        b'/' => (b'/' as u32, false),
        b'?' => (b'/' as u32, true),
        b'`' => (b'`' as u32, false),
        b'~' => (b'`' as u32, true),
        _ => return None,
    })
}

fn send_key<C: Connection>(
    connection: &C,
    root: xproto::Window,
    keycode: u8,
) -> Result<(), GuestProtocolErrorV1> {
    connection
        .xtest_fake_input(
            xproto::KEY_PRESS_EVENT,
            keycode,
            xproto::Time::CURRENT_TIME.into(),
            root,
            0,
            0,
            0,
        )
        .map_err(|_| GuestProtocolErrorV1::Io)?
        .check()
        .map_err(|_| GuestProtocolErrorV1::Io)?;
    send_key_release(connection, root, keycode)
}

fn send_key_release<C: Connection>(
    connection: &C,
    root: xproto::Window,
    keycode: u8,
) -> Result<(), GuestProtocolErrorV1> {
    connection
        .xtest_fake_input(
            xproto::KEY_RELEASE_EVENT,
            keycode,
            xproto::Time::CURRENT_TIME.into(),
            root,
            0,
            0,
            0,
        )
        .map_err(|_| GuestProtocolErrorV1::Io)?
        .check()
        .map_err(|_| GuestProtocolErrorV1::Io)
}

#[cfg(test)]
mod tests {
    use super::ascii_keysym;

    #[test]
    fn guest_text_mapping_is_ascii_only_and_never_builds_shell_arguments() {
        assert_eq!(ascii_keysym('A'), Some((b'a' as u32, true)));
        assert_eq!(ascii_keysym('!'), Some((b'1' as u32, true)));
        assert_eq!(ascii_keysym(' '), Some((b' ' as u32, false)));
        assert_eq!(ascii_keysym('é'), None);
        assert_eq!(ascii_keysym('\n'), None);
    }
}
