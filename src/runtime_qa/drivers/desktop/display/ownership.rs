use x11rb::{
    protocol::{
        res::{ClientIdMask, ClientIdSpec, ConnectionExt as ResConnectionExt},
        xproto::Window,
    },
    rust_connection::RustConnection,
};

use super::display_error;

pub(super) fn window_client_pid(
    connection: &RustConnection,
    id: Window,
) -> Result<Option<u32>, String> {
    let spec = ClientIdSpec {
        client: id,
        mask: ClientIdMask::LOCAL_CLIENT_PID,
    };
    let reply = connection
        .res_query_client_ids(&[spec])
        .map_err(display_error)?
        .reply()
        .map_err(display_error)?;
    Ok(reply
        .ids
        .into_iter()
        .find(|value| value.spec.mask == ClientIdMask::LOCAL_CLIENT_PID)
        .and_then(|value| value.value.first().copied()))
}
