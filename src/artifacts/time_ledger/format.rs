//! Canonical v1 line codec for the git-backed time ledger: strict decode of
//! terminated rows, best-effort salvage of the unterminated final fragment,
//! and the field grammar shared by both (serialization lives beside the row
//! type in the parent module).

use super::{EndStatus, IntervalRow};

/// Strict one-line v1 decode. Returns an explanatory error for any row that is
/// blank, mistagged, mis-columned, ill-formed, or mutually inconsistent.
pub(crate) fn decode_line(line: &str) -> Result<IntervalRow, String> {
    let parts: Vec<&str> = line.split(':').collect();
    if parts.len() != 10 || parts[0] != "T" {
        return Err(format!(
            "expected 9 ':'-separated v1 columns (tag + 9 fields), found {}",
            parts.len()
        ));
    }
    let repo_id = text_field(parts[1], "repo_id")?;
    let workspace_id = text_field(parts[2], "workspace_id")?;
    let session_id = text_field(parts[3], "session_id")?;
    let worker_pid = opt_uint(parts[4], "worker_pid")?;
    let start_epoch_s = uint_field(parts[5], "start_epoch_s")?;
    let end_epoch_s = opt_uint(parts[6], "end_epoch_s")?;
    let item_uid = text_field(parts[7], "item_uid")?;
    let feature_ref = opt_text_field(parts[8], "feature_ref")?;
    let end_status = EndStatus::from_token(parts[9]).ok_or("unknown end_status token")?;
    check_intervals(end_status, start_epoch_s, end_epoch_s)?;
    Ok(IntervalRow {
        repo_id,
        workspace_id,
        session_id,
        worker_pid,
        start_epoch_s,
        end_epoch_s,
        item_uid,
        feature_ref,
        end_status,
    })
}

fn check_intervals(status: EndStatus, start: u64, end: Option<u64>) -> Result<(), String> {
    if matches!(status, EndStatus::Ended | EndStatus::InterruptedCount) && end.is_none() {
        return Err("`ended`/`interrupted-count` rows require an end epoch".to_owned());
    }
    if end.is_some_and(|value| value < start) {
        return Err("end epoch precedes start epoch".to_owned());
    }
    Ok(())
}

fn is_text_raw(raw: &str) -> bool {
    !raw.is_empty()
        && raw
            .chars()
            .all(|c| ('\u{20}'..='\u{7e}').contains(&c) && c != ':' && c != ',')
}

fn text_field(raw: &str, name: &str) -> Result<String, String> {
    is_text_raw(raw)
        .then(|| raw.to_owned())
        .ok_or_else(|| format!("{name} must be non-empty printable text without ':' or ','"))
}

fn opt_text_field(raw: &str, name: &str) -> Result<Option<String>, String> {
    if raw == "-" {
        return Ok(None);
    }
    text_field(raw, name).map(Some)
}

fn digits(raw: &str) -> bool {
    !raw.is_empty() && raw.bytes().all(|b| b.is_ascii_digit())
}

fn uint_field(raw: &str, name: &str) -> Result<u64, String> {
    if !digits(raw) {
        return Err(format!("{name} must be ASCII decimal"));
    }
    raw.parse::<u64>()
        .map_err(|_| format!("{name} exceeds u64 range"))
}

fn opt_uint(raw: &str, name: &str) -> Result<Option<u64>, String> {
    if raw == "-" {
        return Ok(None);
    }
    uint_field(raw, name).map(Some)
}

/// Best-effort [`EndStatus::InterruptedDiscard`] stand-in for the unterminated
/// final fragment: keep the longest strictly decodable prefix, neutralize the
/// rest (count-vs-discard rule in the parent module docs).
pub(crate) fn salvage_torn_fragment(fragment: &str) -> IntervalRow {
    let parts: Vec<&str> = fragment.split(':').collect();
    let text_at = |i: usize| {
        parts
            .get(i)
            .filter(|part| is_text_raw(part))
            .map(|part| part.to_string())
            .unwrap_or_else(|| "?".into())
    };
    let num_at = |i: usize| {
        parts
            .get(i)
            .and_then(|part| digits(part).then(|| part.parse::<u64>().ok()))
            .flatten()
    };
    IntervalRow {
        repo_id: text_at(1),
        workspace_id: text_at(2),
        session_id: text_at(3),
        worker_pid: num_at(4),
        start_epoch_s: num_at(5).unwrap_or(0),
        end_epoch_s: None,
        item_uid: text_at(7),
        feature_ref: None,
        end_status: EndStatus::InterruptedDiscard,
    }
}
