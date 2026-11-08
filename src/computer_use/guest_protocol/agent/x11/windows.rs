use crate::computer_use::{
    ComputerWindowBoundsV1,
    guest_protocol::{GuestObservationV1, GuestProtocolErrorV1, GuestWindowObservationV1},
};
use std::collections::BTreeSet;
use x11rb::{
    connection::Connection,
    protocol::xproto::{AtomEnum, ConnectionExt as _, MapState, Window},
};

const MAX_WINDOWS: usize = 32;
const MAX_TITLE_BYTES: usize = 256;
const MAX_WINDOW_TREE_NODES: usize = 512;

pub(in crate::computer_use::guest_protocol::agent) fn observe_windows(
    session_id: &str,
    previous: &[GuestWindowObservationV1],
) -> Result<Vec<GuestWindowObservationV1>, GuestProtocolErrorV1> {
    let (connection, screen_index) = super::input::connect()?;
    let root = connection.setup().roots[screen_index].root;
    let clients_atom = atom(&connection, b"_NET_CLIENT_LIST")?;
    let listed = connection
        .get_property(
            false,
            root,
            clients_atom,
            AtomEnum::WINDOW,
            0,
            MAX_WINDOWS as u32,
        )
        .map_err(|_| GuestProtocolErrorV1::Io)?
        .reply()
        .map_err(|_| GuestProtocolErrorV1::Io)?
        .value32()
        .map(Iterator::collect::<Vec<_>>)
        .unwrap_or_default();
    let candidates = if listed.is_empty() {
        connection
            .query_tree(root)
            .map_err(|_| GuestProtocolErrorV1::Io)?
            .reply()
            .map_err(|_| GuestProtocolErrorV1::Io)?
            .children
    } else {
        listed
    };
    let focused = connection
        .get_input_focus()
        .map_err(|_| GuestProtocolErrorV1::Io)?
        .reply()
        .map_err(|_| GuestProtocolErrorV1::Io)?
        .focus;
    let mut seen = BTreeSet::new();
    let mut windows = Vec::new();
    for xid in candidates {
        if xid == root || !seen.insert(xid) || windows.len() >= MAX_WINDOWS {
            continue;
        }
        let attributes = match connection
            .get_window_attributes(xid)
            .ok()
            .and_then(|c| c.reply().ok())
        {
            Some(attributes) if attributes.map_state == MapState::VIEWABLE => attributes,
            _ => continue,
        };
        if attributes.override_redirect {
            continue;
        }
        let geometry = match connection
            .get_geometry(xid)
            .ok()
            .and_then(|c| c.reply().ok())
        {
            Some(geometry) if geometry.width > 0 && geometry.height > 0 => geometry,
            _ => continue,
        };
        let translated = connection
            .translate_coordinates(xid, root, 0, 0)
            .map_err(|_| GuestProtocolErrorV1::Io)?
            .reply()
            .map_err(|_| GuestProtocolErrorV1::Io)?;
        let window_id = window_id(session_id, xid)?;
        let bounds = ComputerWindowBoundsV1 {
            left: i32::from(translated.dst_x),
            top: i32::from(translated.dst_y),
            width_px: u32::from(geometry.width),
            height_px: u32::from(geometry.height),
        };
        let is_focused = contains_focus(&connection, xid, focused, root);
        let previous_generation = previous
            .iter()
            .find(|candidate| candidate.window_id == window_id)
            .map(|candidate| {
                if candidate.bounds == bounds && candidate.focused == is_focused {
                    candidate.generation
                } else {
                    candidate.generation.saturating_add(1)
                }
            })
            .unwrap_or(1);
        windows.push(GuestWindowObservationV1 {
            window_id,
            generation: previous_generation,
            bounds,
            title: title(&connection, xid)?,
            app_id: app_id(&connection, xid)?,
            focused: is_focused,
        });
    }
    Ok(windows)
}

fn contains_focus<C: Connection>(
    connection: &C,
    window: Window,
    focused: Window,
    root: Window,
) -> bool {
    let mut current = focused;
    for _ in 0..8 {
        if current == window {
            return true;
        }
        if current == root {
            return false;
        }
        let Ok(cookie) = connection.query_tree(current) else {
            return false;
        };
        let Ok(tree) = cookie.reply() else {
            return false;
        };
        current = tree.parent;
    }
    false
}

pub(in crate::computer_use::guest_protocol::agent) fn resolve_window(
    session_id: &str,
    observation: &GuestObservationV1,
    window_id: &str,
) -> Result<Window, GuestProtocolErrorV1> {
    let expected = observation
        .windows
        .iter()
        .find(|window| window.window_id == window_id)
        .ok_or(GuestProtocolErrorV1::StaleObservation)?;
    let session = uuid::Uuid::parse_str(session_id).map_err(|_| GuestProtocolErrorV1::Malformed)?;
    let (connection, screen_index) = super::input::connect()?;
    let root = connection.setup().roots[screen_index].root;
    let mut remaining = MAX_WINDOW_TREE_NODES;
    let xid = find_window(&connection, root, &session, window_id, 0, &mut remaining)?
        .ok_or(GuestProtocolErrorV1::StaleObservation)?;
    let attributes = connection
        .get_window_attributes(xid)
        .map_err(|_| GuestProtocolErrorV1::Io)?
        .reply()
        .map_err(|_| GuestProtocolErrorV1::Io)?;
    if attributes.map_state != MapState::VIEWABLE || attributes.override_redirect {
        return Err(GuestProtocolErrorV1::StaleObservation);
    }
    let geometry = connection
        .get_geometry(xid)
        .map_err(|_| GuestProtocolErrorV1::Io)?
        .reply()
        .map_err(|_| GuestProtocolErrorV1::Io)?;
    let translated = connection
        .translate_coordinates(xid, root, 0, 0)
        .map_err(|_| GuestProtocolErrorV1::Io)?
        .reply()
        .map_err(|_| GuestProtocolErrorV1::Io)?;
    if u32::from(geometry.width) != expected.bounds.width_px
        || u32::from(geometry.height) != expected.bounds.height_px
        || i32::from(translated.dst_x) != expected.bounds.left
        || i32::from(translated.dst_y) != expected.bounds.top
    {
        return Err(GuestProtocolErrorV1::StaleObservation);
    }
    Ok(xid)
}

fn find_window<C: Connection>(
    connection: &C,
    parent: Window,
    session: &uuid::Uuid,
    expected_id: &str,
    depth: usize,
    remaining: &mut usize,
) -> Result<Option<Window>, GuestProtocolErrorV1> {
    if depth > 8 || *remaining == 0 {
        return Ok(None);
    }
    let children = connection
        .query_tree(parent)
        .map_err(|_| GuestProtocolErrorV1::Io)?
        .reply()
        .map_err(|_| GuestProtocolErrorV1::Io)?
        .children;
    for child in children {
        if *remaining == 0 {
            break;
        }
        *remaining -= 1;
        let id = uuid::Uuid::new_v5(
            session,
            format!("computer-use-window-v1:{child}").as_bytes(),
        )
        .to_string();
        if id == expected_id {
            return Ok(Some(child));
        }
        if let Some(found) = find_window(
            connection,
            child,
            session,
            expected_id,
            depth + 1,
            remaining,
        )? {
            return Ok(Some(found));
        }
    }
    Ok(None)
}

fn atom<C: Connection>(connection: &C, name: &[u8]) -> Result<u32, GuestProtocolErrorV1> {
    connection
        .intern_atom(false, name)
        .map_err(|_| GuestProtocolErrorV1::Io)?
        .reply()
        .map(|reply| reply.atom)
        .map_err(|_| GuestProtocolErrorV1::Io)
}

fn title<C: Connection>(connection: &C, window: Window) -> Result<String, GuestProtocolErrorV1> {
    let property = atom(connection, b"_NET_WM_NAME")?;
    let bytes = connection
        .get_property(
            false,
            window,
            property,
            AtomEnum::ANY,
            0,
            MAX_TITLE_BYTES as u32,
        )
        .map_err(|_| GuestProtocolErrorV1::Io)?
        .reply()
        .map_err(|_| GuestProtocolErrorV1::Io)?
        .value;
    Ok(truncate_title(&String::from_utf8_lossy(&bytes)))
}

fn truncate_title(value: &str) -> String {
    let mut output = String::new();
    for character in value.chars().filter(|character| !character.is_control()) {
        if output.len() + character.len_utf8() > MAX_TITLE_BYTES {
            break;
        }
        output.push(character);
    }
    output
}

fn app_id<C: Connection>(
    connection: &C,
    window: Window,
) -> Result<Option<String>, GuestProtocolErrorV1> {
    let property = connection
        .get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 64)
        .map_err(|_| GuestProtocolErrorV1::Io)?
        .reply()
        .map_err(|_| GuestProtocolErrorV1::Io)?
        .value;
    let class = String::from_utf8_lossy(&property).to_ascii_lowercase();
    Ok([
        ("chromium", "chromium"),
        ("thunar", "file-manager"),
        ("mousepad", "text-editor"),
        ("computer-use-fixture", "test-gui-fixture"),
    ]
    .into_iter()
    .find_map(|(needle, id)| class.contains(needle).then(|| id.to_owned())))
}

fn window_id(session_id: &str, xid: Window) -> Result<String, GuestProtocolErrorV1> {
    let namespace =
        uuid::Uuid::parse_str(session_id).map_err(|_| GuestProtocolErrorV1::Malformed)?;
    Ok(uuid::Uuid::new_v5(
        &namespace,
        format!("computer-use-window-v1:{xid}").as_bytes(),
    )
    .to_string())
}
