use std::{fs, io::Cursor, path::Path};

use tar::{Builder, Header};

pub(super) struct Fixture(pub(super) std::path::PathBuf);

impl Fixture {
    pub(super) fn new() -> Self {
        let path = std::env::temp_dir().join(format!("koolade-stage-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(path.join(".git")).unwrap();
        fs::write(path.join("source.txt"), b"original").unwrap();
        Self(path)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(super) fn returned(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut builder = Builder::new(Vec::new());
    for (path, bytes) in entries {
        let mut header = Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_path(path).unwrap();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append(&header, Cursor::new(bytes)).unwrap();
    }
    builder.into_inner().unwrap()
}

pub(super) fn returned_with_mode(path: &str, bytes: &[u8], mode: u32) -> Vec<u8> {
    let mut builder = Builder::new(Vec::new());
    let mut header = Header::new_gnu();
    header.set_entry_type(tar::EntryType::Regular);
    header.set_path(path).unwrap();
    header.set_size(bytes.len() as u64);
    header.set_mode(mode);
    header.set_cksum();
    builder.append(&header, Cursor::new(bytes)).unwrap();
    builder.into_inner().unwrap()
}

pub(super) fn returned_with_hard_link(path: &str, source: &str, target: &str) -> Vec<u8> {
    let mut builder = Builder::new(Vec::new());
    let mut file = Header::new_gnu();
    file.set_entry_type(tar::EntryType::Regular);
    file.set_path(source).unwrap();
    file.set_size(7);
    file.set_mode(0o644);
    file.set_cksum();
    builder.append(&file, Cursor::new(b"payload")).unwrap();
    let mut link = Header::new_gnu();
    link.set_entry_type(tar::EntryType::Link);
    link.set_path(path).unwrap();
    link.set_link_name(target).unwrap();
    link.set_size(0);
    link.set_mode(0o644);
    link.set_cksum();
    builder.append(&link, Cursor::new([])).unwrap();
    builder.into_inner().unwrap()
}

pub(super) fn traversal_returned(path: &str, bytes: &[u8]) -> Vec<u8> {
    let mut builder = Builder::new(Vec::new());
    let mut header = Header::new_gnu();
    let name = path.as_bytes();
    assert!(name.len() < 100);
    header.as_mut_bytes()[..name.len()].copy_from_slice(name);
    header.set_entry_type(tar::EntryType::Regular);
    header.set_size(bytes.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder.append(&header, Cursor::new(bytes)).unwrap();
    builder.into_inner().unwrap()
}

pub(super) fn git(root: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}
