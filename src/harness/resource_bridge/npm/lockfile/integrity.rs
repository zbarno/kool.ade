use sha2::{Digest, Sha512};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub(crate) fn npm_cache_digest_path(cache: &Path, integrity: &str) -> Option<PathBuf> {
    let sri = integrity
        .split_whitespace()
        .find_map(|token| token.strip_prefix("sha512-"))?;
    let encoded = sri.split_once('?').map_or(sri, |(digest, _)| digest);
    let digest = decode_base64(encoded)?;
    if digest.len() != 64 {
        return None;
    }
    let hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Some(
        cache
            .join("_cacache/content-v2/sha512")
            .join(&hex[..2])
            .join(&hex[2..4])
            .join(&hex[4..]),
    )
}

pub(crate) fn verify_sha512_file(path: &Path, integrity: &str) -> bool {
    let Some(encoded) = integrity
        .split_whitespace()
        .find_map(|token| token.strip_prefix("sha512-"))
    else {
        return false;
    };
    let encoded = encoded
        .split_once('?')
        .map_or(encoded, |(digest, _)| digest);
    let Some(expected) = decode_base64(encoded) else {
        return false;
    };
    if expected.len() != 64 {
        return false;
    }
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    Sha512::digest(bytes).as_slice() == expected
}

#[cfg(test)]
pub(crate) fn cache_covers_lockfile(worktree: &Path, cache: &Path) -> bool {
    let Ok(packages) = super::collect_lockfiles(worktree) else {
        return false;
    };
    packages.iter().all(|package| {
        npm_cache_digest_path(cache, &package.integrity).is_some_and(|path| {
            fs::symlink_metadata(path)
                .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
        })
    })
}

pub(crate) fn decode_base64(value: &str) -> Option<Vec<u8>> {
    fn sextet(byte: u8) -> Option<u8> {
        match byte {
            b'A'..=b'Z' => Some(byte - b'A'),
            b'a'..=b'z' => Some(byte - b'a' + 26),
            b'0'..=b'9' => Some(byte - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    if value.len() != 88 || !value.ends_with("==") || value[..86].contains('=') {
        return None;
    }
    let mut out = Vec::with_capacity(64);
    let mut accumulator = 0_u32;
    let mut bits = 0_u8;
    for byte in value.bytes().take(86) {
        accumulator = (accumulator << 6) | u32::from(sextet(byte)?);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((accumulator >> bits) & 0xff) as u8);
        }
    }
    (out.len() == 64 && accumulator & 0x0f == 0).then_some(out)
}
