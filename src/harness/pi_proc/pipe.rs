use std::io::BufRead;

use super::StreamEvt;

/// Read one pipe into line events until EOF or the event consumer goes away.
pub(super) fn pipe_lines<R: std::io::Read + Send + 'static>(
    reader: R,
    tx: std::sync::mpsc::Sender<StreamEvt>,
    is_stdout: bool,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut br = std::io::BufReader::new(reader);
        let mut line = String::new();
        loop {
            line.clear();
            match br.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let text = line.trim_end().to_string();
                    let event = if is_stdout {
                        StreamEvt::Stdout(text)
                    } else {
                        StreamEvt::Stderr(text)
                    };
                    if tx.send(event).is_err() {
                        break;
                    }
                }
            }
        }
    })
}
