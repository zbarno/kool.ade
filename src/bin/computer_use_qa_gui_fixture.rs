#[cfg(target_os = "linux")]
use x11rb::{
    COPY_DEPTH_FROM_PARENT,
    connection::Connection,
    protocol::xproto::{
        AtomEnum, ConnectionExt, CreateGCAux, CreateWindowAux, EventMask, Rectangle, WindowClass,
    },
    wrapper::ConnectionExt as WrapperConnectionExt,
};

#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let defect_fixture = env!("CARGO_BIN_NAME").contains("defect");
    let network_state = if network_probes_are_blocked() {
        "network probes blocked"
    } else {
        "network probe unexpectedly connected"
    };
    let artifact_state = if defect_fixture {
        ""
    } else {
        prepare_artifact_symlink_fixture()?;
        "; artifact symlink fixture ready"
    };
    let (connection, screen_index) = x11rb::connect(None)?;
    let screen = &connection.setup().roots[screen_index];
    let window = connection.generate_id()?;
    connection.create_window(
        COPY_DEPTH_FROM_PARENT,
        window,
        screen.root,
        0,
        0,
        360,
        180,
        0,
        WindowClass::INPUT_OUTPUT,
        screen.root_visual,
        &CreateWindowAux::new()
            .background_pixel(screen.white_pixel)
            .event_mask(EventMask::EXPOSURE | EventMask::BUTTON_PRESS | EventMask::KEY_PRESS),
    )?;
    let gc = connection.generate_id()?;
    connection.create_gc(
        gc,
        window,
        &CreateGCAux::new().foreground(screen.black_pixel),
    )?;
    connection.change_property8(
        x11rb::protocol::xproto::PropMode::REPLACE,
        window,
        AtomEnum::WM_CLASS,
        AtomEnum::STRING,
        b"computer-use-fixture\0computer-use-fixture",
    )?;
    set_title(
        &connection,
        window,
        &format!("wrong; {network_state}{artifact_state}"),
    )?;
    connection.map_window(window)?;
    connection.flush()?;

    loop {
        match connection.wait_for_event()? {
            x11rb::protocol::Event::Expose(_) => {
                connection.poly_rectangle(
                    window,
                    gc,
                    &[Rectangle {
                        x: 20,
                        y: 45,
                        width: 260,
                        height: 72,
                    }],
                )?;
                connection.image_text8(window, gc, 38, 85, b"Click to verify destination")?;
                connection.flush()?;
            }
            x11rb::protocol::Event::ButtonPress(_) if !defect_fixture => {
                set_title(&connection, window, "expected")?;
            }
            x11rb::protocol::Event::KeyPress(_) => break,
            _ => {}
        }
    }
    connection.free_gc(gc)?;
    connection.destroy_window(window)?;
    connection.flush()?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn prepare_artifact_symlink_fixture() -> Result<(), Box<dyn std::error::Error>> {
    use std::{fs, os::unix::fs::symlink, path::Path};

    let downloads = Path::new("/home/koolade/Downloads");
    fs::create_dir_all(downloads)?;
    fs::write(
        downloads.join("koolade-bridge-sentinel.txt"),
        b"original guest sentinel",
    )?;
    symlink(
        downloads.join("koolade-bridge-sentinel.txt"),
        downloads.join("koolade-bridge-link.txt"),
    )?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn network_probes_are_blocked() -> bool {
    use std::{
        net::{SocketAddr, TcpStream},
        time::Duration,
    };

    // The real-KVM acceptance additionally checks QMP's effective device graph
    // and denies network hotplug. These bounded probes exercise guest TCP
    // reachability for a public endpoint, the link-local metadata address, and
    // QEMU's conventional user-mode gateway from the launched QA application.
    ["1.1.1.1:443", "169.254.169.254:80", "10.0.2.2:22"]
        .into_iter()
        .all(|target| {
            target.parse::<SocketAddr>().ok().is_some_and(|address| {
                TcpStream::connect_timeout(&address, Duration::from_millis(250)).is_err()
            })
        })
}

#[cfg(target_os = "linux")]
fn set_title(
    connection: &impl Connection,
    window: u32,
    destination: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let title = format!("Computer QA destination /{destination}");
    let net_wm_name = connection
        .intern_atom(false, b"_NET_WM_NAME")?
        .reply()?
        .atom;
    let utf8_string = connection.intern_atom(false, b"UTF8_STRING")?.reply()?.atom;
    connection.change_property8(
        x11rb::protocol::xproto::PropMode::REPLACE,
        window,
        net_wm_name,
        utf8_string,
        title.as_bytes(),
    )?;
    connection.change_property8(
        x11rb::protocol::xproto::PropMode::REPLACE,
        window,
        AtomEnum::WM_NAME,
        AtomEnum::STRING,
        title.as_bytes(),
    )?;
    connection.flush()?;
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("this Computer Use QA fixture is only supported on Linux");
    std::process::exit(1);
}
