use crate::computer_use::{MouseButtonV1, guest_protocol::GuestProtocolErrorV1};
use std::{thread, time::Duration};
use x11rb::{
    connection::Connection,
    protocol::{
        xproto::{self, ConnectionExt as _, Window},
        xtest::ConnectionExt as _,
    },
};

pub(in crate::computer_use::guest_protocol::agent) fn click(
    x: u32,
    y: u32,
    button: MouseButtonV1,
) -> Result<(), GuestProtocolErrorV1> {
    let (connection, screen_index) = super::connect()?;
    let root = connection.setup().roots[screen_index].root;
    validate_point(&connection, root, x, y)?;
    move_on(&connection, root, x, y)?;
    click_on(&connection, root, button)
}

pub(in crate::computer_use::guest_protocol::agent) fn double_click(
    x: u32,
    y: u32,
    button: MouseButtonV1,
) -> Result<(), GuestProtocolErrorV1> {
    click(x, y, button)?;
    thread::sleep(Duration::from_millis(80));
    click(x, y, button)
}

pub(in crate::computer_use::guest_protocol::agent) fn move_pointer(
    x: u32,
    y: u32,
) -> Result<(), GuestProtocolErrorV1> {
    let (connection, screen_index) = super::connect()?;
    let root = connection.setup().roots[screen_index].root;
    validate_point(&connection, root, x, y)?;
    move_on(&connection, root, x, y)
}

pub(in crate::computer_use::guest_protocol::agent) fn drag(
    from_x: u32,
    from_y: u32,
    to_x: u32,
    to_y: u32,
    button: MouseButtonV1,
) -> Result<(), GuestProtocolErrorV1> {
    let (connection, screen_index) = super::connect()?;
    let root = connection.setup().roots[screen_index].root;
    validate_point(&connection, root, from_x, from_y)?;
    validate_point(&connection, root, to_x, to_y)?;
    move_on(&connection, root, from_x, from_y)?;
    let button = button_code(button);
    let pressed = send_button(&connection, root, xproto::BUTTON_PRESS_EVENT, button);
    let moved = if pressed.is_ok() {
        move_on(&connection, root, to_x, to_y)
    } else {
        Ok(())
    };
    let released = send_button(&connection, root, xproto::BUTTON_RELEASE_EVENT, button);
    pressed.and(moved).and(released)
}

pub(in crate::computer_use::guest_protocol::agent) fn scroll(
    x: u32,
    y: u32,
    delta_x: i16,
    delta_y: i16,
) -> Result<(), GuestProtocolErrorV1> {
    let (connection, screen_index) = super::connect()?;
    let root = connection.setup().roots[screen_index].root;
    validate_point(&connection, root, x, y)?;
    move_on(&connection, root, x, y)?;
    let vertical = if delta_y < 0 { 5 } else { 4 };
    let horizontal = if delta_x < 0 { 7 } else { 6 };
    for _ in 0..delta_y.unsigned_abs() {
        send_click(&connection, root, vertical)?;
    }
    for _ in 0..delta_x.unsigned_abs() {
        send_click(&connection, root, horizontal)?;
    }
    Ok(())
}

pub(in crate::computer_use::guest_protocol::agent) fn focus_window(
    window: Window,
) -> Result<(), GuestProtocolErrorV1> {
    let (connection, _) = super::connect()?;
    connection
        .set_input_focus(
            xproto::InputFocus::PARENT,
            window,
            xproto::Time::CURRENT_TIME,
        )
        .map_err(|_| GuestProtocolErrorV1::Io)?
        .check()
        .map_err(|_| GuestProtocolErrorV1::Io)
}

fn validate_point<C: Connection>(
    connection: &C,
    root: Window,
    x: u32,
    y: u32,
) -> Result<(), GuestProtocolErrorV1> {
    let geometry = connection
        .get_geometry(root)
        .map_err(|_| GuestProtocolErrorV1::Io)?
        .reply()
        .map_err(|_| GuestProtocolErrorV1::Io)?;
    if x >= u32::from(geometry.width) || y >= u32::from(geometry.height) {
        return Err(GuestProtocolErrorV1::StaleObservation);
    }
    Ok(())
}

fn move_on<C: Connection>(
    connection: &C,
    root: Window,
    x: u32,
    y: u32,
) -> Result<(), GuestProtocolErrorV1> {
    connection
        .xtest_fake_input(
            xproto::MOTION_NOTIFY_EVENT,
            0,
            xproto::Time::CURRENT_TIME.into(),
            root,
            x as i16,
            y as i16,
            0,
        )
        .map_err(|_| GuestProtocolErrorV1::Io)?
        .check()
        .map_err(|_| GuestProtocolErrorV1::Io)
}

fn click_on<C: Connection>(
    connection: &C,
    root: Window,
    button: MouseButtonV1,
) -> Result<(), GuestProtocolErrorV1> {
    send_click(connection, root, button_code(button))
}

fn send_click<C: Connection>(
    connection: &C,
    root: Window,
    button: u8,
) -> Result<(), GuestProtocolErrorV1> {
    let pressed = send_button(connection, root, xproto::BUTTON_PRESS_EVENT, button);
    let released = send_button(connection, root, xproto::BUTTON_RELEASE_EVENT, button);
    pressed.and(released)
}

fn send_button<C: Connection>(
    connection: &C,
    root: Window,
    event: u8,
    button: u8,
) -> Result<(), GuestProtocolErrorV1> {
    connection
        .xtest_fake_input(
            event,
            button,
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

fn button_code(button: MouseButtonV1) -> u8 {
    match button {
        MouseButtonV1::Primary => 1,
        MouseButtonV1::Middle => 2,
        MouseButtonV1::Secondary => 3,
    }
}
