use crate::computer_use::{ComputerKeyV1, guest_protocol::GuestProtocolErrorV1};
use x11rb::{
    connection::Connection,
    protocol::{
        xproto::{self, ConnectionExt as _},
        xtest::ConnectionExt as _,
    },
};

pub(in crate::computer_use::guest_protocol::agent) fn type_text(
    text: &str,
) -> Result<(), GuestProtocolErrorV1> {
    let (connection, screen_index) = super::connect()?;
    let root = connection.setup().roots[screen_index].root;
    let keymap = super::keyboard_map(&connection)?;
    let shift = super::keycode_for_keysym_from_map(&keymap, 0xffe1, false)?;
    let unicode_keycode = free_keycode(&connection, &keymap)?;
    let keys = text
        .chars()
        .map(|ch| {
            let (sym, shifted) = match ch {
                '\n' => (0xff0d, false),
                '\t' => (0xff09, false),
                ch => match super::ascii_keysym(ch) {
                    Some(mapping) => mapping,
                    None => return Ok(TextKey::Unicode(ch as u32)),
                },
            };
            match super::keycode_for_keysym_from_map(&keymap, sym, shifted) {
                Ok(code) => Ok(TextKey::Mapped(code, shifted)),
                Err(_) => Ok(TextKey::Unicode(ch as u32)),
            }
        })
        .collect::<Result<Vec<_>, GuestProtocolErrorV1>>()?;
    for key in keys {
        match key {
            TextKey::Mapped(code, shifted) => {
                tap_key(&connection, root, code, shifted.then_some(shift))?;
            }
            TextKey::Unicode(codepoint) => {
                type_unicode(&connection, root, unicode_keycode, codepoint, &keymap)?;
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum TextKey {
    Mapped(u8, bool),
    Unicode(u32),
}

fn free_keycode<C: Connection>(
    connection: &C,
    keymap: &xproto::GetKeyboardMappingReply,
) -> Result<u8, GuestProtocolErrorV1> {
    let per_key = keymap.keysyms_per_keycode as usize;
    let index = keymap
        .keysyms
        .chunks_exact(per_key)
        .position(|symbols| symbols.iter().all(|symbol| *symbol == 0))
        .ok_or(GuestProtocolErrorV1::UnsupportedCapability)?;
    Ok(connection.setup().min_keycode.saturating_add(index as u8))
}

fn type_unicode<C: Connection>(
    connection: &C,
    root: xproto::Window,
    keycode: u8,
    codepoint: u32,
    original_map: &xproto::GetKeyboardMappingReply,
) -> Result<(), GuestProtocolErrorV1> {
    let per_key = original_map.keysyms_per_keycode;
    let start = usize::from(keycode.saturating_sub(connection.setup().min_keycode))
        .checked_mul(usize::from(per_key))
        .ok_or(GuestProtocolErrorV1::Malformed)?;
    let original = original_map
        .keysyms
        .get(start..start + usize::from(per_key))
        .ok_or(GuestProtocolErrorV1::Malformed)?;
    let mut temporary = vec![0; usize::from(per_key)];
    temporary[0] = 0x0100_0000 | codepoint;
    let grabbed = connection
        .grab_server()
        .map_err(|_| GuestProtocolErrorV1::Io)
        .and_then(|cookie| cookie.check().map_err(|_| GuestProtocolErrorV1::Io));
    if let Err(error) = grabbed {
        let _ = connection.ungrab_server();
        return Err(error);
    }
    let changed = connection
        .change_keyboard_mapping(1, keycode, per_key, &temporary)
        .map_err(|_| GuestProtocolErrorV1::Io)
        .and_then(|cookie| cookie.check().map_err(|_| GuestProtocolErrorV1::Io));
    let sent = changed.and_then(|()| tap_key(connection, root, keycode, None));
    let restored = connection
        .change_keyboard_mapping(1, keycode, per_key, original)
        .map_err(|_| GuestProtocolErrorV1::Io)
        .and_then(|cookie| cookie.check().map_err(|_| GuestProtocolErrorV1::Io));
    let ungrabbed = connection
        .ungrab_server()
        .map_err(|_| GuestProtocolErrorV1::Io)
        .and_then(|cookie| cookie.check().map_err(|_| GuestProtocolErrorV1::Io));
    sent.and(restored).and(ungrabbed)
}

pub(in crate::computer_use::guest_protocol::agent) fn key_chord(
    keys: &[ComputerKeyV1],
) -> Result<(), GuestProtocolErrorV1> {
    if keys.is_empty() || keys.len() > 8 {
        return Err(GuestProtocolErrorV1::Malformed);
    }
    let (connection, screen_index) = super::connect()?;
    let root = connection.setup().roots[screen_index].root;
    let keymap = super::keyboard_map(&connection)?;
    let codes = keys
        .iter()
        .map(|key| {
            let keysym = match key {
                ComputerKeyV1::Named(key) => Ok(super::named_keysym(*key)),
                ComputerKeyV1::Character(ch) => super::ascii_keysym(*ch)
                    .map(|(sym, _)| sym)
                    .ok_or(GuestProtocolErrorV1::UnsupportedCapability),
            }?;
            super::keycode_for_keysym_from_map(&keymap, keysym, false)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut pressed = Vec::with_capacity(codes.len());
    for code in codes {
        if let Err(error) = key_event(&connection, root, code, true) {
            let _ = key_event(&connection, root, code, false);
            for active in pressed.iter().rev() {
                let _ = key_event(&connection, root, *active, false);
            }
            return Err(error);
        }
        pressed.push(code);
    }
    let mut release_error = None;
    for code in pressed.iter().rev() {
        if let Err(error) = key_event(&connection, root, *code, false) {
            release_error.get_or_insert(error);
        }
    }
    release_error.map_or(Ok(()), Err)
}

fn tap_key<C: Connection>(
    connection: &C,
    root: xproto::Window,
    code: u8,
    modifier: Option<u8>,
) -> Result<(), GuestProtocolErrorV1> {
    if let Some(modifier) = modifier {
        let pressed_modifier = key_event(connection, root, modifier, true);
        let key = if pressed_modifier.is_ok() {
            let pressed_key = key_event(connection, root, code, true);
            let released_key = key_event(connection, root, code, false);
            pressed_key.and(released_key)
        } else {
            Ok(())
        };
        let released_modifier = key_event(connection, root, modifier, false);
        pressed_modifier.and(key).and(released_modifier)
    } else {
        let pressed = key_event(connection, root, code, true);
        let released = key_event(connection, root, code, false);
        pressed.and(released)
    }
}

fn key_event<C: Connection>(
    c: &C,
    root: xproto::Window,
    code: u8,
    pressed: bool,
) -> Result<(), GuestProtocolErrorV1> {
    c.xtest_fake_input(
        if pressed {
            xproto::KEY_PRESS_EVENT
        } else {
            xproto::KEY_RELEASE_EVENT
        },
        code,
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
