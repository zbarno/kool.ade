//! Session-scoped public HTTPS tunnel for opt-in InternetResearch guests.
//!
//! The guest can reach this proxy only through QEMU's restricted guestfwd
//! endpoint. DNS resolution and the pinned public socket are owned by the
//! controller; direct guest traffic has no route to the host or the Internet.

mod policy;
mod proxy;
#[cfg(test)]
mod tests;

use std::{
    io,
    net::{Shutdown, SocketAddr, TcpListener},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

const MAX_PARALLEL_CONNECTIONS: usize = 8;
const MAX_TOTAL_CONNECTIONS: u64 = 256;

struct ConnectionSlot(Arc<AtomicUsize>);

impl Drop for ConnectionSlot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Release);
    }
}

pub(super) struct InternetResearchGatewayV1 {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    epoch: Arc<AtomicU64>,
    worker: Option<thread::JoinHandle<()>>,
}

impl InternetResearchGatewayV1 {
    pub(super) fn start(lifetime: Duration) -> io::Result<Self> {
        if lifetime.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "InternetResearch lifetime is empty",
            ));
        }
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?;
        let stop = Arc::new(AtomicBool::new(false));
        let paused = Arc::new(AtomicBool::new(false));
        let epoch = Arc::new(AtomicU64::new(0));
        let worker_stop = stop.clone();
        let worker_paused = paused.clone();
        let worker_epoch = epoch.clone();
        let worker = thread::Builder::new()
            .name("koolade-computer-research-gateway".into())
            .spawn(move || serve(listener, worker_stop, worker_paused, worker_epoch, lifetime))?;
        Ok(Self {
            address,
            stop,
            paused,
            epoch,
            worker: Some(worker),
        })
    }

    pub(super) fn address(&self) -> SocketAddr {
        self.address
    }

    pub(super) fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }

    pub(super) fn pause(&self) {
        self.paused.store(true, Ordering::Release);
        self.epoch.fetch_add(1, Ordering::AcqRel);
    }

    pub(super) fn resume(&self) {
        if !self.stop.load(Ordering::Acquire) {
            self.paused.store(false, Ordering::Release);
        }
    }
}

impl Drop for InternetResearchGatewayV1 {
    fn drop(&mut self) {
        self.stop();
    }
}

fn serve(
    listener: TcpListener,
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    epoch: Arc<AtomicU64>,
    lifetime: Duration,
) {
    let started = Instant::now();
    let active = Arc::new(AtomicUsize::new(0));
    let connections = Arc::new(AtomicU64::new(0));
    let transferred = Arc::new(AtomicU64::new(0));
    while !stop.load(Ordering::Acquire) && started.elapsed() < lifetime {
        match listener.accept() {
            Ok((client, _)) => {
                if paused.load(Ordering::Acquire) {
                    let _ = client.shutdown(Shutdown::Both);
                    continue;
                }
                let connection_number = connections.fetch_add(1, Ordering::AcqRel);
                if connection_number >= MAX_TOTAL_CONNECTIONS || !acquire_slot(&active) {
                    let _ = client.shutdown(Shutdown::Both);
                    continue;
                }
                let worker_stop = stop.clone();
                let worker_paused = paused.clone();
                let worker_epoch = epoch.clone();
                let worker_generation = epoch.load(Ordering::Acquire);
                let worker_active = active.clone();
                let worker_transferred = transferred.clone();
                let spawned = thread::Builder::new()
                    .name("koolade-computer-research-connection".into())
                    .spawn(move || {
                        let _slot = ConnectionSlot(worker_active);
                        let _ = proxy::serve_connection(
                            client,
                            proxy::TunnelState::new(
                                worker_stop,
                                worker_paused,
                                worker_epoch,
                                worker_generation,
                                started,
                                lifetime,
                                worker_transferred,
                            ),
                        );
                    });
                if spawned.is_err() {
                    active.fetch_sub(1, Ordering::Release);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(_) => break,
        }
    }
    stop.store(true, Ordering::Release);
}

fn acquire_slot(active: &AtomicUsize) -> bool {
    active
        .fetch_update(Ordering::AcqRel, Ordering::Relaxed, |count| {
            (count < MAX_PARALLEL_CONNECTIONS).then_some(count + 1)
        })
        .is_ok()
}
