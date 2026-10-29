#[path = "x11/api.rs"]
mod api;

use super::state::AppState;
use api::Event;

pub(super) fn run() -> Result<(), String> {
    let client = api::Client::open()?;
    let mut state = AppState::default();
    client.render(&state.title())?;
    loop {
        match client.next_event()? {
            Event::Expose => {}
            Event::Click => state.click(),
            Event::Text(text) => state.type_text(&text),
            Event::Ignore => continue,
        }
        client.render(&state.title())?;
    }
}
