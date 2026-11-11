use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use crate::computer_use::ComputerBackendErrorV1 as Error;

const CGROUP_CONTROLLERS: &[&str] = &["cpu", "memory", "pids"];

pub(crate) struct CgroupV2 {
    root: PathBuf,
}

impl CgroupV2 {
    pub(crate) fn new(root: &Path) -> Result<Self, Error> {
        let metadata = fs::symlink_metadata(root).map_err(|_| Error::HostNotReady)?;
        if !metadata.file_type().is_dir() || !root.join("cgroup.controllers").is_file() {
            return Err(Error::HostNotReady);
        }
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    pub(crate) fn probe(&self) -> Result<(), Error> {
        let controllers = fs::read_to_string(self.root.join("cgroup.controllers"))
            .map_err(|_| Error::HostNotReady)?;
        let enabled = fs::read_to_string(self.root.join("cgroup.subtree_control"))
            .map_err(|_| Error::HostNotReady)?;
        if CGROUP_CONTROLLERS.iter().any(|controller| {
            !controllers
                .split_ascii_whitespace()
                .any(|entry| entry == *controller)
        }) || CGROUP_CONTROLLERS.iter().any(|controller| {
            !enabled
                .split_ascii_whitespace()
                .any(|entry| entry.trim_start_matches('+') == *controller)
        }) || !self.root.join("cgroup.procs").exists()
        {
            return Err(Error::HostNotReady);
        }
        Ok(())
    }

    pub(crate) fn create(
        &self,
        session: &crate::computer_use::ComputerSessionV1,
        max_processes: u16,
    ) -> Result<(), Error> {
        self.probe()?;
        let path = self.path(&session.identity.session_id);
        fs::create_dir(&path).map_err(|error| {
            #[cfg(test)]
            if std::env::var("KOOLADE_COMPUTER_USE_R1_REAL_KVM").as_deref() == Ok("1") {
                eprintln!("R1 cgroup create failed: {error}");
            }
            #[cfg(not(test))]
            let _ = error;
            Error::HostNotReady
        })?;
        let budget = &session.policy.resource_budget;
        let quota = u64::from(budget.cpu_millis).saturating_mul(100);
        let limits = (|| {
            write_control(
                &path.join("cpu.max"),
                format!("{quota} 100000\n").as_bytes(),
            )?;
            write_control(
                &path.join("memory.max"),
                format!("{}\n", budget.memory_bytes).as_bytes(),
            )?;
            write_control(&path.join("memory.swap.max"), b"0\n")?;
            write_control(&path.join("memory.oom.group"), b"1\n")?;
            write_control(
                &path.join("pids.max"),
                format!("{max_processes}\n").as_bytes(),
            )
        })();
        if limits.is_err() {
            let _ = fs::remove_dir(&path);
        }
        limits
    }

    pub(crate) fn process_path(&self, session_id: &str) -> PathBuf {
        self.path(session_id).join("cgroup.procs")
    }

    pub(crate) fn kill_and_remove(&self, session_id: &str) -> Result<(), Error> {
        let path = self.path(session_id);
        if !path.exists() {
            return Ok(());
        }
        if path.join("cgroup.kill").exists() {
            let _ = write_control(&path.join("cgroup.kill"), b"1\n");
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if fs::read_to_string(path.join("cgroup.procs"))
                .is_ok_and(|pids| pids.trim().is_empty())
            {
                return fs::remove_dir(path).map_err(|_| Error::Io);
            }
            thread::sleep(Duration::from_millis(25));
        }
        Err(Error::Io)
    }

    fn path(&self, session_id: &str) -> PathBuf {
        self.root.join(format!("koolade-{session_id}"))
    }
}

fn write_control(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let result = OpenOptions::new()
        .write(true)
        .open(path)
        .and_then(|mut file| file.write_all(bytes));
    result.map_err(|error| {
        #[cfg(test)]
        if std::env::var("KOOLADE_COMPUTER_USE_R1_REAL_KVM").as_deref() == Ok("1") {
            eprintln!("R1 cgroup write failed: {error}");
        }
        #[cfg(not(test))]
        let _ = error;
        Error::HostNotReady
    })
}
