use std::{env, thread, time::Duration};

mod state;
mod x11;

fn main() {
    if env::var("DISPLAY").map_or(true, |display| display.trim().is_empty()) {
        // The Rust executor has no display; ProcessAlive is its approved readiness signal.
        loop {
            thread::park_timeout(Duration::from_secs(60));
        }
    }
    if let Err(error) = x11::run() {
        eprintln!("Rust desktop fixture failed: {error}");
        std::process::exit(1);
    }
}
