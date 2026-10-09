use std::{
    io::{self, Read},
    os::unix::net::UnixStream,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

pub(super) struct ClientDisconnectMonitor {
    disconnected: Arc<AtomicBool>,
    complete: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl ClientDisconnectMonitor {
    pub(super) fn start(client: &UnixStream) -> io::Result<Self> {
        let mut probe = client.try_clone()?;
        probe.set_read_timeout(Some(Duration::from_millis(100)))?;
        let disconnected = Arc::new(AtomicBool::new(false));
        let complete = Arc::new(AtomicBool::new(false));
        let worker_disconnected = disconnected.clone();
        let worker_complete = complete.clone();
        let worker = thread::spawn(move || {
            let mut byte = [0_u8; 1];
            loop {
                match probe.read(&mut byte) {
                    Ok(0) => {
                        worker_disconnected.store(true, Ordering::Release);
                        return;
                    }
                    Ok(_) => {
                        worker_disconnected.store(true, Ordering::Release);
                        return;
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                        ) =>
                    {
                        if worker_complete.load(Ordering::Acquire) {
                            return;
                        }
                    }
                    Err(_) => {
                        worker_disconnected.store(true, Ordering::Release);
                        return;
                    }
                }
            }
        });
        Ok(Self {
            disconnected,
            complete,
            worker: Some(worker),
        })
    }

    pub(super) fn disconnected(&self) -> &AtomicBool {
        &self.disconnected
    }
}

impl Drop for ClientDisconnectMonitor {
    fn drop(&mut self) {
        self.complete.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
