//! Read-time per-workspace day and ISO-week totals (F7, scope 4 / R4, AD-8).
//!
//! Pure aggregation over ledger [`IntervalRow`]s loaded from the git-backed
//! time ledger (F7 task 1): no IO, no mutation, no network (R6). Board
//! surfaces (F7 task 6) and the month report (F7 task 5) must both derive
//! from [`day_totals`]/[`week_totals`] so the exported-equals-displayed-
//! equals-summed invariant holds from one source.
//!
//! Rules encoded (R3, R4, AD-2, AD-5, AD-8):
//!
//! * **Billable set** — [`EndStatus::Ended`] and `InterruptedCount` rows
//!   accrue; `InterruptedDiscard` rows and rows without an end epoch
//!   contribute nothing (the ledger's count-versus-discard rule).
//! * **Union on overlap** — overlapping intervals of one workspace merge
//!   before summing, so a workspace's day total never exceeds one times
//!   wall rate for that day; distinct workspaces aggregate independently
//!   and may accrue concurrently (AD-5).
//! * **Device-local civil dates** with no DST correction (AD-2), and
//!   ISO-8601 Monday-through-Sunday weeks identified by their ISO
//!   (year, week) pair ([`IsoWeek`], AD-8).
//!
//! The in-progress day appears as soon as any row ends inside it: the
//! accrual engine (F7 task 2) closes intervals at every stoppage, so the
//! partial accumulation rides in with its rows and this module never reads
//! the wall clock. Results are thus determined solely by the input rows:
//! identical rows in, byte-identical serialized maps out, with no empty
//! buckets invented (a missing (period, workspace) cell reads as zero).
//!
//! Week buckets are derived from the same per-day pieces, so per-day and
//! per-week totals always reconcile with each other (R3/AC3). Complexity is
//! linear in rows times days spanned; epochs are device wall seconds
//! (AD-2 corrects anomalous clocks).

use std::collections::BTreeMap;
use std::fmt;

use chrono::{
    DateTime, Datelike, IsoWeek as ChronoIsoWeek, Local, NaiveDate, NaiveDateTime, TimeZone,
};
use serde::Serialize;

use crate::artifacts::time_ledger::{EndStatus, IntervalRow};

/// Seconds accrued by individual workspaces for one period: workspace id
/// mapped to whole wall seconds, in deterministic (workspace id) order.
pub type WorkspaceSeconds = BTreeMap<String, u64>;

/// Device-local civil date mapped to per-workspace seconds, in date order.
pub type DayTotals = BTreeMap<NaiveDate, WorkspaceSeconds>;

/// One ISO-8601 week (Monday through Sunday, AD-8) by its ISO year and
/// week number. The ISO year is the year containing the week's Thursday;
/// week numbers run 1..=53. Ordered chronologically by (year, week);
/// serializes as `YYYY-Www` (e.g. `2024-W03`).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct IsoWeek {
    /// ISO year of the week (year containing its Thursday).
    pub year: i32,
    /// ISO week number, 1..=53.
    pub week: u32,
}

impl fmt::Display for IsoWeek {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-W{:02}", self.year, self.week)
    }
}

impl Serialize for IsoWeek {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// ISO week mapped to per-workspace seconds, in week order.
pub type WeekTotals = BTreeMap<IsoWeek, WorkspaceSeconds>;

/// Per-workspace seconds for every device-local civil day covered by
/// `rows`, including the in-progress day once rows end in it. Overlaps
/// within one workspace count once; discarded and unfinished rows accrue
/// nothing.
pub fn day_totals(rows: &[IntervalRow]) -> DayTotals {
    let (days, _) = split_by_period(rows);
    collapse_periods(days)
}

/// Per-workspace seconds for every ISO-8601 week (Mon-Sun) covered by
/// `rows`. Derived from the same per-day pieces as [`day_totals`], so the
/// two views reconcile exactly.
pub fn week_totals(rows: &[IntervalRow]) -> WeekTotals {
    let (_, weeks) = split_by_period(rows);
    collapse_periods(weeks)
}

#[cfg(test)]
mod tests;

// ----- internals ----------------------------------------------------------

type Segments<K> = BTreeMap<K, BTreeMap<String, Vec<(i64, i64)>>>;

/// Billable slice of a row: the workspace plus a strictly increasing pair
/// of whole-second instants (half-open). Discarded rows, unfinished rows
/// (no end epoch), and zero-length rows contribute nothing.
fn billed_span(row: &IntervalRow) -> Option<(&str, u64, u64)> {
    let end = row.end_epoch_s?;
    if matches!(row.end_status, EndStatus::InterruptedDiscard) || end <= row.start_epoch_s {
        return None;
    }
    Some((row.workspace_id.as_str(), row.start_epoch_s, end))
}

/// Device-local civil date of one whole-second device-wall instant; `None`
/// when the instant is outside chrono's representable range.
fn device_date(epoch_s: u64) -> Option<NaiveDate> {
    let secs = i64::try_from(epoch_s).ok()?;
    let instant = DateTime::from_timestamp(secs, 0)?;
    let utc_wall = instant.naive_utc();
    Some(Local.from_utc_datetime(&utc_wall).naive_local().date())
}

/// Whole-second device boundary interval `[lo, hi)` of one civil day. The
/// wall midnights map through the device-local zone (no DST correction per
/// AD-2); for degenerate zones the nearest representable instant wins.
fn day_bounds(day: NaiveDate) -> (i64, i64) {
    let lo = wall_to_epoch(day.and_hms_opt(0, 0, 0).expect("midnight is always valid"));
    let hi = day
        .succ_opt()
        .and_then(|next| next.and_hms_opt(0, 0, 0).map(wall_to_epoch))
        .expect("next civil day outside representable range");
    (lo, hi)
}

fn wall_to_epoch(wall: NaiveDateTime) -> i64 {
    Local
        .from_local_datetime(&wall)
        .earliest()
        .or_else(|| Local.from_local_datetime(&wall).latest())
        .expect("device zone defines no such wall instant (broken TZ data; AD-2)")
        .timestamp()
}

fn iso_week_of(day: NaiveDate) -> IsoWeek {
    let iso: ChronoIsoWeek = day.iso_week();
    IsoWeek {
        year: iso.year(),
        week: iso.week(),
    }
}

/// Splits every billable row into per-covered-civil-day pieces and files
/// each piece under both its day and its ISO week. Pieces are half-open
/// device-epoch intervals that never cross a day boundary.
fn split_by_period(rows: &[IntervalRow]) -> (Segments<NaiveDate>, Segments<IsoWeek>) {
    let (mut days, mut weeks) = (Segments::new(), Segments::new());
    for row in rows {
        let Some((workspace, start, end)) = billed_span(row) else {
            continue;
        };
        let Some(start_i) = i64::try_from(start).ok() else {
            continue;
        };
        let Some(end_i) = i64::try_from(end).ok() else {
            continue;
        };
        let Some(first_day) = device_date(start) else {
            continue;
        };
        // `end > start >= 0`, so `end - 1` is the last covered second.
        let Some(last_day) = device_date(end - 1) else {
            continue;
        };
        let mut day = first_day;
        while day <= last_day {
            let (lo, hi) = day_bounds(day);
            let (piece_start, piece_end) = (start_i.max(lo), end_i.min(hi));
            if piece_start < piece_end {
                let owned = workspace.to_owned();
                let piece = (piece_start, piece_end);
                days.entry(day)
                    .or_default()
                    .entry(owned.clone())
                    .or_default()
                    .push(piece);
                weeks
                    .entry(iso_week_of(day))
                    .or_default()
                    .entry(owned)
                    .or_default()
                    .push(piece);
            }
            day = match day.succ_opt() {
                Some(next) => next,
                None => break,
            };
        }
    }
    (days, weeks)
}

/// Collapses each (period, workspace) span collection to the seconds of its
/// union, dropping empty workspaces and then empty periods.
fn collapse_periods<K: Ord>(segments: Segments<K>) -> BTreeMap<K, WorkspaceSeconds> {
    segments
        .into_iter()
        .map(|(period, workspaces)| {
            let seconds = workspaces
                .into_iter()
                .map(|(workspace, mut spans)| {
                    spans.sort_unstable();
                    (workspace, union_seconds(&spans))
                })
                .filter(|(_, total)| *total > 0)
                .collect::<WorkspaceSeconds>();
            (period, seconds)
        })
        .filter(|(_, seconds)| !seconds.is_empty())
        .collect()
}

/// Whole seconds covered by the union of non-empty sorted half-open
/// `[start, end)` spans (callers sort before calling).
fn union_seconds(spans: &[(i64, i64)]) -> u64 {
    let (mut cover_start, mut cover_end) = spans[0];
    let mut seconds: i64 = 0;
    for &(start, end) in &spans[1..] {
        if start > cover_end {
            seconds += cover_end - cover_start;
            (cover_start, cover_end) = (start, end);
        } else {
            cover_end = cover_end.max(end);
        }
    }
    seconds += cover_end - cover_start;
    u64::try_from(seconds.max(0)).expect("union length is non-negative")
}
