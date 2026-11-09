use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        Arc, Condvar, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

const MAX_QUEUED_SESSIONS: usize = 32;
const MAX_QUEUE_WAIT: Duration = Duration::from_secs(15 * 60);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ResourceRequest {
    pub vcpus: u16,
    pub memory_bytes: u64,
    pub disk_bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Capacity {
    slots: u16,
    vcpus: u16,
    memory_bytes: u64,
    disk_bytes: u64,
}

#[derive(Default)]
struct State {
    next_ticket: u64,
    waiting: VecDeque<Waiter>,
    active: BTreeMap<String, ResourceRequest>,
}

#[derive(Clone)]
struct Waiter {
    ticket: u64,
    session_id: String,
    cancelled: Arc<AtomicBool>,
}

pub(super) struct ComputerUseSchedulerV1 {
    capacity: Capacity,
    state: Mutex<State>,
    changed: Condvar,
}

pub(super) struct SessionPermit {
    scheduler: Weak<ComputerUseSchedulerV1>,
    session_id: String,
}

impl ComputerUseSchedulerV1 {
    pub fn new(slots: u16, vcpus: u16, memory_bytes: u64, disk_bytes: u64) -> Option<Arc<Self>> {
        (slots > 0 && vcpus > 0 && memory_bytes > 0 && disk_bytes > 0).then(|| {
            Arc::new(Self {
                capacity: Capacity {
                    slots,
                    vcpus,
                    memory_bytes,
                    disk_bytes,
                },
                state: Mutex::new(State::default()),
                changed: Condvar::new(),
            })
        })
    }

    pub fn acquire(
        self: &Arc<Self>,
        session_id: &str,
        request: ResourceRequest,
        cancelled: &AtomicBool,
    ) -> Result<SessionPermit, &'static str> {
        if session_id.is_empty()
            || request.vcpus == 0
            || request.memory_bytes == 0
            || request.disk_bytes == 0
            || request.vcpus > self.capacity.vcpus
            || request.memory_bytes > self.capacity.memory_bytes
            || request.disk_bytes > self.capacity.disk_bytes
        {
            return Err("session_resource_budget_exceeds_host_capacity");
        }
        let deadline = Instant::now() + MAX_QUEUE_WAIT;
        let mut state = self
            .state
            .lock()
            .map_err(|_| "session_scheduler_unavailable")?;
        if state.active.contains_key(session_id)
            || state
                .waiting
                .iter()
                .any(|waiter| waiter.session_id == session_id)
        {
            return Err("session_already_scheduled");
        }
        if state.waiting.len() >= MAX_QUEUED_SESSIONS {
            return Err("computer_session_queue_full");
        }
        let ticket = state.next_ticket;
        state.next_ticket = state
            .next_ticket
            .checked_add(1)
            .ok_or("session_scheduler_exhausted")?;
        let waiter_cancelled = Arc::new(AtomicBool::new(false));
        state.waiting.push_back(Waiter {
            ticket,
            session_id: session_id.to_owned(),
            cancelled: waiter_cancelled.clone(),
        });

        loop {
            if cancelled.load(Ordering::Acquire) || waiter_cancelled.load(Ordering::Acquire) {
                remove_waiter(&mut state, ticket);
                self.changed.notify_all();
                return Err("cancelled");
            }
            if Instant::now() >= deadline {
                remove_waiter(&mut state, ticket);
                self.changed.notify_all();
                return Err("computer_session_queue_expired");
            }
            let is_head = state
                .waiting
                .front()
                .is_some_and(|waiter| waiter.ticket == ticket);
            if is_head && fits(self.capacity, &state, request) {
                let waiter = state.waiting.pop_front().expect("head was checked");
                state.active.insert(waiter.session_id.clone(), request);
                self.changed.notify_all();
                return Ok(SessionPermit {
                    scheduler: Arc::downgrade(self),
                    session_id: waiter.session_id,
                });
            }
            let wait_for = deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(100));
            let (next, _) = self
                .changed
                .wait_timeout(state, wait_for)
                .map_err(|_| "session_scheduler_unavailable")?;
            state = next;
        }
    }

    pub fn cancel_waiting(&self, session_id: &str) -> bool {
        let Ok(state) = self.state.lock() else {
            return false;
        };
        let Some(waiter) = state
            .waiting
            .iter()
            .find(|waiter| waiter.session_id == session_id)
        else {
            return false;
        };
        waiter.cancelled.store(true, Ordering::Release);
        self.changed.notify_all();
        true
    }

    pub fn queue_position(&self, session_id: &str) -> Option<usize> {
        let state = self.state.lock().ok()?;
        state
            .waiting
            .iter()
            .position(|waiter| waiter.session_id == session_id)
            .map(|position| position + 1)
    }

    #[cfg(test)]
    fn counts(&self) -> (usize, usize) {
        let state = self.state.lock().unwrap();
        (state.active.len(), state.waiting.len())
    }
}

impl Drop for SessionPermit {
    fn drop(&mut self) {
        let Some(scheduler) = self.scheduler.upgrade() else {
            return;
        };
        if let Ok(mut state) = scheduler.state.lock() {
            state.active.remove(&self.session_id);
            scheduler.changed.notify_all();
        }
    }
}

fn fits(capacity: Capacity, state: &State, request: ResourceRequest) -> bool {
    if state.active.len() >= capacity.slots as usize {
        return false;
    }
    let used = state.active.values().fold((0u32, 0u64, 0u64), |sum, item| {
        (
            sum.0.saturating_add(item.vcpus as u32),
            sum.1.saturating_add(item.memory_bytes),
            sum.2.saturating_add(item.disk_bytes),
        )
    });
    used.0.saturating_add(request.vcpus as u32) <= capacity.vcpus as u32
        && used.1.saturating_add(request.memory_bytes) <= capacity.memory_bytes
        && used.2.saturating_add(request.disk_bytes) <= capacity.disk_bytes
}

fn remove_waiter(state: &mut State, ticket: u64) {
    state.waiting.retain(|waiter| waiter.ticket != ticket);
}

#[cfg(test)]
#[path = "scheduler/tests.rs"]
mod tests;
