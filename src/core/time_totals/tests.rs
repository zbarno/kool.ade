//! Deterministic unit coverage for `super`: boundary splits, union-on-overlap
//! (R3), ISO-week anchoring (AD-8), in-progress day (R4), purity, and the
//! billable-set exclusions.

use super::*;
use crate::artifacts::time_ledger::{EndStatus, IntervalRow};
use crate::core::time_accrual::unix_now_seconds;
use chrono::{Datelike, NaiveDate, Weekday};
use std::collections::BTreeSet;

const Y: i32 = 2024; // 2024-01-01 was a Monday; ISO W01 = 2024-01-01 .. 2024-01-07.

fn row(workspace: &str, start: i64, end: i64) -> IntervalRow {
    IntervalRow {
        repo_id: "root".into(),
        workspace_id: workspace.to_owned(),
        session_id: format!("s{start}e{end}"),
        worker_pid: Some(4242),
        start_epoch_s: start.max(0) as u64,
        end_epoch_s: Some(end.max(0) as u64),
        item_uid: format!("T{start}-{end}"),
        feature_ref: None,
        end_status: EndStatus::Ended,
    }
}

fn discarded_row(workspace: &str, start: i64, end: i64) -> IntervalRow {
    let mut r = row(workspace, start, end);
    r.end_status = EndStatus::InterruptedDiscard;
    r
}

fn unfinished_row(workspace: &str, start: i64) -> IntervalRow {
    let mut r = row(workspace, start, start);
    r.end_epoch_s = None;
    r.end_status = EndStatus::InterruptedDiscard;
    r
}

/// Device-local epoch for a civil fixture instant.
fn wall(day: NaiveDate, hour: i64, minute: i64) -> i64 {
    wall_to_epoch(
        day.and_hms_opt(hour as u32, minute as u32, 0)
            .expect("fixture time"),
    )
}

fn md(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(Y, 1, day).expect("fixture date")
}

fn hm(base: i64, hour: i64, minute: i64) -> i64 {
    base + hour * 3600 + minute * 60
}

fn wk(iso_year: i32, week: u32) -> IsoWeek {
    IsoWeek {
        year: iso_year,
        week,
    }
}

fn wp(pairs: &[(&str, u64)]) -> BTreeMap<String, u64> {
    pairs
        .iter()
        .map(|(ws, secs)| (ws.to_string(), *secs))
        .collect()
}

fn cell(map: &BTreeMap<String, u64>, workspace: &str) -> Option<u64> {
    map.get(workspace).copied()
}

#[test]
fn fixture_weekday_and_iso_anchors_hold() {
    assert_eq!(md(3).weekday(), Weekday::Wed);
    assert_eq!(md(6).weekday(), Weekday::Sat);
    assert_eq!(md(7).weekday(), Weekday::Sun);
    assert_eq!(md(8).weekday(), Weekday::Mon);
    assert_eq!(iso_week_of(md(3)), wk(2024, 1));
    assert_eq!(iso_week_of(md(7)), wk(2024, 1));
    assert_eq!(
        iso_week_of(md(8)),
        wk(2024, 2),
        "Monday opens the next ISO week"
    );
    assert_eq!(wk(2024, 2).to_string(), "2024-W02");
}

#[test]
fn interval_straddling_midnight_splits_into_each_day() {
    let sat = wall(md(6), 23, 0);
    let rows = vec![row("alpha", sat, sat + 2 * 3600)]; // Sat 23:00 -> Sun 01:00
    let days = day_totals(&rows);
    let keys: Vec<NaiveDate> = days.keys().copied().collect();
    assert_eq!(keys, vec![md(6), md(7)]);
    assert_eq!(days.get(&md(6)), Some(&wp(&[("alpha", 3600)])));
    assert_eq!(days.get(&md(7)), Some(&wp(&[("alpha", 3600)])));
    let weeks = week_totals(&rows);
    let wkeys: Vec<IsoWeek> = weeks.keys().copied().collect();
    assert_eq!(wkeys, vec![wk(2024, 1)]);
    assert_eq!(weeks.get(&wk(2024, 1)), Some(&wp(&[("alpha", 7200)])));
}

#[test]
fn sun_to_mon_interval_lands_in_two_adjacent_iso_weeks() {
    let sat23 = wall(md(6), 23, 0);
    let rows = vec![row("beta", sat23, sat23 + 26 * 3600)]; // Sat 23:00 -> Mon 01:00
    let days = day_totals(&rows);
    let dkeys: Vec<NaiveDate> = days.keys().copied().collect();
    assert_eq!(dkeys, vec![md(6), md(7), md(8)]);
    assert_eq!(days.get(&md(6)), Some(&wp(&[("beta", 3600)])));
    assert_eq!(days.get(&md(7)), Some(&wp(&[("beta", 86_400)])));
    assert_eq!(days.get(&md(8)), Some(&wp(&[("beta", 3600)])));
    let weeks = week_totals(&rows);
    let wkeys: Vec<IsoWeek> = weeks.keys().copied().collect();
    assert_eq!(wkeys, vec![wk(2024, 1), wk(2024, 2)]);
    assert_eq!(
        weeks.get(&wk(2024, 1)),
        Some(&wp(&[("beta", 3600 + 86_400)]))
    );
    assert_eq!(weeks.get(&wk(2024, 2)), Some(&wp(&[("beta", 3600)])));
}

#[test]
fn overlapping_same_workspace_intervals_merge_to_union_length() {
    let d3 = wall(md(3), 0, 0);
    let rows = vec![
        row("alpha", hm(d3, 9, 0), hm(d3, 10, 0)),
        row("alpha", hm(d3, 9, 30), hm(d3, 10, 30)), // overlaps previous
        row("alpha", hm(d3, 9, 0), hm(d3, 10, 0)),   // duplicate: counts once
        row("alpha", hm(d3, 9, 5), hm(d3, 9, 15)),   // nested: adds nothing
        row("alpha", hm(d3, 11, 0), hm(d3, 11, 30)),
    ];
    let days = day_totals(&rows);
    let dkeys: Vec<NaiveDate> = days.keys().copied().collect();
    assert_eq!(dkeys, vec![md(3)]);
    let got = cell(days.get(&md(3)).unwrap(), "alpha").expect("alpha present");
    assert_eq!(
        got,
        90 * 60 + 30 * 60,
        "union of 09:00-10:30 (5400s) and 11:00-11:30 (1800s)"
    );
    assert!(
        got <= 9000,
        "never exceeds wall seconds between 09:00 and 11:30"
    );
}

#[test]
fn concurrent_workspaces_accrue_independently() {
    let d3 = wall(md(3), 0, 0);
    let rows = vec![
        row("alpha", hm(d3, 9, 0), hm(d3, 10, 0)),
        row("beta", hm(d3, 9, 30), hm(d3, 10, 30)), // temporally overlapping, other workspace
    ];
    let days = day_totals(&rows);
    assert_eq!(
        days.get(&md(3)),
        Some(&wp(&[("alpha", 3600), ("beta", 3600)])),
        "full spans per workspace despite temporal overlap"
    );
}

#[test]
fn exact_consecutive_seconds_report_exactly() {
    let d3 = wall(md(3), 0, 0);
    let rows = vec![row("alpha", hm(d3, 12, 0), hm(d3, 12, 0) + 300)];
    let days = day_totals(&rows);
    assert_eq!(cell(days.get(&md(3)).unwrap(), "alpha"), Some(300));
}

/// The day that carries the freshest recorded second is always present in
/// the map (the in-progress day, R4), holding exactly its partial share.
/// Anchors on the row's own newest instant rather than a second wall read,
/// so a midnight rollover between the two reads cannot shake the test.
#[test]
fn in_progress_day_appears_with_partial_seconds() {
    let now = i64::try_from(unix_now_seconds()).expect("modern clock");
    let rows = vec![row("gamma", now - 900, now)];
    let days = day_totals(&rows);
    let in_progress = device_date(u64::try_from(now - 1).expect("post-1970 clock"))
        .expect("representable civil date");
    assert!(
        days.contains_key(&in_progress),
        "the day carrying the freshest recorded second must be present: {days:?}"
    );
    let total: u64 = days.values().flat_map(|m| m.values()).copied().sum();
    assert_eq!(total, 900, "whole row accounted for across days");
    let (lo, _hi) = day_bounds(in_progress);
    let expected_today = (now - lo).clamp(1, 900);
    assert_eq!(
        cell(days.get(&in_progress).unwrap(), "gamma"),
        Some(expected_today as u64),
        "the in-progress day carries the row\u{2019}s [max(start, day_start), now] share"
    );
}

#[test]
fn identical_input_yields_byte_identical_serialization() {
    let d3 = wall(md(3), 0, 0);
    let sat = wall(md(6), 23, 0);
    let rows: Vec<IntervalRow> = vec![
        row("alpha", hm(d3, 9, 0), hm(d3, 10, 30)),
        row("beta", sat, sat + 7 * 3600),
        row("alpha", hm(d3, 11, 0), hm(d3, 11, 30)),
    ];
    let (d1, w1) = (day_totals(&rows), week_totals(&rows));
    let (d2, w2) = (day_totals(&rows), week_totals(&rows));
    assert_eq!(
        serde_json::to_string(&d1).unwrap(),
        serde_json::to_string(&d2).unwrap()
    );
    assert_eq!(
        serde_json::to_string(&w1).unwrap(),
        serde_json::to_string(&w2).unwrap()
    );
    let reversed: Vec<IntervalRow> = rows.iter().rev().cloned().collect();
    assert_eq!(
        serde_json::to_string(&d1).unwrap(),
        serde_json::to_string(&day_totals(&reversed)).unwrap(),
        "ordering of the input vector is immaterial"
    );
}

#[test]
fn rows_outside_a_period_do_not_leak_into_neighboring_buckets() {
    let d3 = wall(md(3), 0, 0);
    let contained = vec![row("alpha", hm(d3, 10, 0), hm(d3, 10, 30))];
    let days = day_totals(&contained);
    let dkeys: Vec<NaiveDate> = days.keys().copied().collect();
    assert_eq!(dkeys, vec![md(3)]);
    let wkeys: Vec<IsoWeek> = week_totals(&contained).keys().copied().collect();
    assert_eq!(wkeys, vec![wk(2024, 1)]);

    let boundary = vec![row("alpha", hm(d3, 23, 30), hm(d3, 0, 30) + 24 * 3600)]; // Wed 23:30 -> Thu 00:30
    let days = day_totals(&boundary);
    let dkeys: Vec<NaiveDate> = days.keys().copied().collect();
    assert_eq!(dkeys, vec![md(3), md(4)], "no third day invented");
    assert_eq!(days.get(&md(3)), Some(&wp(&[("alpha", 1800)])));
    assert_eq!(days.get(&md(4)), Some(&wp(&[("alpha", 1800)])));
    let wkeys: Vec<IsoWeek> = week_totals(&boundary).keys().copied().collect();
    assert_eq!(wkeys, vec![wk(2024, 1)]);
}

#[test]
fn discarded_and_unfinished_rows_accrue_nothing() {
    let d3 = wall(md(3), 0, 0);
    let rows = vec![
        discarded_row("alpha", hm(d3, 10, 0), hm(d3, 11, 0)),
        unfinished_row("gamma", hm(d3, 11, 0)),
        row("beta", hm(d3, 10, 0), hm(d3, 11, 0)),
    ];
    let days = day_totals(&rows);
    let dkeys: Vec<NaiveDate> = days.keys().copied().collect();
    assert_eq!(dkeys, vec![md(3)]);
    assert_eq!(
        days.get(&md(3)),
        Some(&wp(&[("beta", 3600)])),
        "only the Ended row accrues"
    );
}

#[test]
fn multi_day_interval_splits_into_every_day_it_touches() {
    let thur30 = wall(md(4), 0, 30);
    let rows = vec![row("theta", thur30, thur30 + 3 * 86_400)]; // Thu 00:30 -> Sun 00:30
    let days = day_totals(&rows);
    let dkeys: Vec<NaiveDate> = days.keys().copied().collect();
    assert_eq!(dkeys, vec![md(4), md(5), md(6), md(7)]);
    assert_eq!(days.get(&md(4)), Some(&wp(&[("theta", 84_600)])));
    assert_eq!(days.get(&md(5)), Some(&wp(&[("theta", 86_400)])));
    assert_eq!(days.get(&md(6)), Some(&wp(&[("theta", 86_400)])));
    assert_eq!(days.get(&md(7)), Some(&wp(&[("theta", 1800)])));
    let weeks = week_totals(&rows);
    let wkeys: Vec<IsoWeek> = weeks.keys().copied().collect();
    assert_eq!(wkeys, vec![wk(2024, 1)]);
    assert_eq!(weeks.get(&wk(2024, 1)), Some(&wp(&[("theta", 3 * 86_400)])));
}
/// Randomized differential check: the module's totals against a per-second
/// brute-force union, over a 72-hour window that spans two midnights AND the
/// Sun->Mon ISO-week seam (late W01 -> early W02 of 2024). Fixed seed makes
/// the input a reproducible regression corpus; brute force assumes nothing
/// the module assumes, so agreement is strong mutual evidence for cross-day
/// splits, week anchoring, union-on-overlap, and the billable-set rules.
#[test]
fn randomized_totals_match_per_second_bruteforce_union() {
    let mut state: u64 = 0xc0dec0df_f7a11c1a;
    let mut rnd = move || {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 33) as usize
    };
    let ws_names = ["alpha", "beta"];
    let friday_noon = wall(md(5), 12, 0);
    let window: i64 = 72 * 3_600;
    let statuses = [
        EndStatus::Ended,
        EndStatus::InterruptedCount,
        EndStatus::InterruptedDiscard,
    ];

    let mut covered: BTreeMap<NaiveDate, BTreeMap<String, BTreeSet<i64>>> = BTreeMap::new();
    let mut rows: Vec<IntervalRow> = Vec::new();
    for _ in 0..60 {
        let ws = ws_names[rnd() % ws_names.len()].to_owned();
        let status = statuses[rnd() % statuses.len()];
        let start = friday_noon - window / 2 + (rnd() % (window as usize)) as i64;
        let end = if rnd() % 7 == 0 {
            None
        } else {
            Some(start + 1 + (rnd() % (6 * 3_600) as usize) as i64)
        };
        let billable = matches!(status, EndStatus::Ended | EndStatus::InterruptedCount)
            && end.filter(|&e| e > start).is_some();
        if let (true, Some(end)) = (billable, end) {
            for s in start..end {
                let day = device_date(s as u64).expect("in fixture range");
                covered
                    .entry(day)
                    .or_default()
                    .entry(ws.clone())
                    .or_default()
                    .insert(s);
            }
        }
        let mut r = row(&ws, start, end.unwrap_or(start));
        r.end_epoch_s = end.map(|e| e as u64);
        r.end_status = status;
        rows.push(r);
    }

    let want_days: DayTotals = covered
        .into_iter()
        .map(|(day, mut wss)| {
            wss.retain(|_, secs| !secs.is_empty());
            (
                day,
                wss.into_iter()
                    .map(|(ws, secs)| (ws, secs.len() as u64))
                    .collect::<WorkspaceSeconds>(),
            )
        })
        .filter(|(_, wss)| !wss.is_empty())
        .collect();
    assert_eq!(
        day_totals(&rows),
        want_days,
        "day totals diverge from per-second union"
    );

    let mut want_weeks: BTreeMap<IsoWeek, BTreeMap<String, u64>> = BTreeMap::new();
    for (day, wss) in &want_days {
        for (ws, secs) in wss {
            *want_weeks
                .entry(iso_week_of(*day))
                .or_default()
                .entry(ws.clone())
                .or_default() += secs;
        }
    }
    assert_eq!(
        week_totals(&rows),
        want_weeks,
        "week totals diverge from day-summed union"
    );
}
