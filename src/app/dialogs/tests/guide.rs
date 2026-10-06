use super::super::super::dialogs::{
    GuideLine, GuideLineKind, ProbeReport, ProbeView, drain_probe, harness_guide_lines,
};
use super::super::*;
use super::support::{project_from, tempdir};
// ---- F-16 guide golden-text / probe pins (NFR-8, D-15) -------------

use GuideLineKind as K;

fn gl(k: K, t: &str) -> GuideLine {
    GuideLine {
        kind: k,
        text: t.into(),
    }
}

fn rep(status: &str, diagnostic: &str, binary: Option<&str>, ok: bool) -> ProbeReport {
    ProbeReport {
        status: status.into(),
        diagnostic: diagnostic.into(),
        binary: binary.map(std::path::PathBuf::from),
        ok,
        configuration_required: false,
    }
}

/// GOLDEN — found state: exact line count, ordering, and byte-identical
/// text for every line, including the five discovery lines, the override
/// naming, the version-policy rule, and the npm step. Any wording or
/// reorder breakage fails this test (the D-15 render pin).
#[test]
fn harness_guide_golden_found_state_pins_every_line() {
    let view = ProbeView::Report(rep("pi 0.84.4", "", Some("/home/op/.local/bin/pi"), true));
    let lines = harness_guide_lines(&view, Some("/home/op"));
    let expected = vec![
        gl(K::Title, "Set up the pi harness"),
        gl(
            K::Lead,
            "Kool.ad/e shells out to a locally installed pi CLI; it downloads and \
             installs nothing itself.",
        ),
        gl(K::Status, "pi 0.84.4"),
        gl(K::Detail, "Winning binary: /home/op/.local/bin/pi"),
        gl(K::Rule, "Discovery order — first match wins:"),
        gl(
            K::Order,
            "1. KOOLADE_PI_BIN override: if set and executable it wins outright; \
             a bad value fails fast with no fall-through.",
        ),
        gl(K::Order, "2. pi in every PATH directory, in PATH order."),
        gl(K::Order, "3. /home/op/.npm-global/bin/pi"),
        gl(K::Order, "4. /home/op/.local/bin/pi"),
        gl(K::Order, "5. /home/op/.pi/bin/pi"),
        gl(
            K::Rule,
            "Version policy: no floor, no pinning — any installed pi is \
             accepted; the probed version is display-only (D-13).",
        ),
        gl(K::Lead, "Install & make discoverable:"),
        gl(
            K::Step,
            "1. Obtain pi via the vendor channel — npm install -g \
             @earendil-works/pi-coding-agent (adjust if the vendor's \
             documented channel differs).",
        ),
        gl(
            K::Step,
            "2. Make it reachable via PATH, a home location above, or \
             KOOLADE_PI_BIN=/abs/path/to/pi in the launching environment.",
        ),
        gl(
            K::Step,
            "3. Reopen this dialog and confirm the status reads 'pi \
             <version>'.",
        ),
    ];
    assert_eq!(lines, expected, "guide copy drifted from the D-15 pin");
}

/// GOLDEN — unavailable state: the status line pins the DANGER-state
/// string verbatim and the Detail line is the diagnostic itself, with no
/// “Winning binary” claim (fast-fail semantics preserved in the copy).
#[test]
fn harness_guide_golden_unavailable_state_pins_danger_status_and_diagnostic() {
    let view = ProbeView::Report(rep(
        "pi (unavailable: Pi harness not found)",
        "KOOLADE_PI_BIN=/nonexistent-koolade-selftest/pi is not an executable file",
        None,
        false,
    ));
    let lines = harness_guide_lines(&view, Some("/home/op"));
    let status = lines
        .iter()
        .find(|l| l.kind == K::Status)
        .expect("status line present");
    assert_eq!(status.text, "pi (unavailable: Pi harness not found)");
    let detail = lines
        .iter()
        .find(|l| l.kind == K::Detail)
        .expect("detail line present for a report");
    assert_eq!(
        detail.text,
        "KOOLADE_PI_BIN=/nonexistent-koolade-selftest/pi is not an executable file"
    );
    // Fail-fast honesty: no line may promise a PATH rescue for a bad
    // override.
    assert!(
        !lines
            .iter()
            .any(|l| l.text.contains("would have") || l.text.contains("fallback"))
    );
}

/// PENDING state + HOME-less composition: the first paint shows the
/// placeholder status and NO Detail line; with HOME unset the order
/// lines render literal $HOME (mirroring locate_binary skipping home
/// sites) and nothing else changes shape.
#[test]
fn harness_guide_pending_state_pins_placeholder_and_absent_detail() {
    let lines = harness_guide_lines(&ProbeView::Pending, None);
    let status = lines
        .iter()
        .find(|l| l.kind == K::Status)
        .expect("status line present");
    assert_eq!(status.text, "Looking for the pi CLI\u{2026}");
    assert!(
        !lines.iter().any(|l| l.kind == K::Detail),
        "pending state must not show a detail line: {lines:?}"
    );
    let orders: Vec<&GuideLine> = lines.iter().filter(|l| l.kind == K::Order).collect();
    assert_eq!(orders.len(), 5, "five discovery tiers");
    assert_eq!(orders[2].text, "3. $HOME/.npm-global/bin/pi");
    assert_eq!(orders[3].text, "4. $HOME/.local/bin/pi");
    assert_eq!(orders[4].text, "5. $HOME/.pi/bin/pi");
    assert!(
        orders[0].text.starts_with("1."),
        "env override stays tier 1: {}",
        orders[0].text
    );
    assert!(
        orders[0].text.contains("KOOLADE_PI_BIN"),
        "{}",
        orders[0].text
    );
    assert_eq!(lines.len(), 14, "pending drops exactly the detail line");
}

/// Single-source pin: the composed discovery lines derive FROM the
/// harness constants — guards against re-hardcoding the order or env
/// name in the UI copy (reorder/rename in code or copy breaks this).
#[test]
fn harness_guide_discovery_lines_single_source_from_harness_constants() {
    use crate::harness::pi_harness::{COMMON_HOME_SITES, PI_BINARY_ENV};
    let view = ProbeView::Report(rep("pi 1.2.3", "", Some("/x/pi"), true));
    let lines = harness_guide_lines(&view, Some("/h"));
    let orders: Vec<&GuideLine> = lines.iter().filter(|l| l.kind == K::Order).collect();
    assert_eq!(orders.len(), 2 + COMMON_HOME_SITES.len());
    assert!(
        orders[0].text.contains(PI_BINARY_ENV),
        "tier 1 must name {}: {}",
        PI_BINARY_ENV,
        orders[0].text
    );
    assert_eq!(
        orders[1].text,
        "2. pi in every PATH directory, in PATH order."
    );
    for (i, site) in COMMON_HOME_SITES.iter().enumerate() {
        assert_eq!(
            orders[i + 2].text,
            format!("{}. /h/{site}/pi", i + 3),
            "site #{i} drifted from COMMON_HOME_SITES"
        );
    }
    // The step offering the override also derives from the constant.
    let steps: Vec<&GuideLine> = lines.iter().filter(|l| l.kind == K::Step).collect();
    assert!(
        steps[1].text.contains(PI_BINARY_ENV),
        "step 2: {}",
        steps[1].text
    );
}

/// Runtime leg (headless stand-in for the manual smoke): opening the
/// dialog spawns the detached probe, and the drain path used by
/// `paint_harness_guide` observes the pending → report transition within
/// the probe's ~12 s budget — no dialog reopen, no keyboard input. The
/// assertions are host-agnostic (hold whether pi is installed or not).
#[test]
fn opened_dialog_flips_pending_to_report_within_probe_budget() {
    let root = tempdir("probe-flip");
    let proj = project_from(&root);
    let mut dlg = DlgSettings::from_project(&proj);
    assert!(
        matches!(dlg.probe_view, ProbeView::Pending),
        "fresh open is pending before the probe replies"
    );
    // Pump exactly like the painter: drain, then let the UI cadence
    // elapse. Bound generously past the ~12 s worst-case probe.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(25);
    loop {
        drain_probe(&mut dlg);
        if matches!(dlg.probe_view, ProbeView::Report(_)) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "probe did not report within budget; view still: {:?}",
            dlg.probe_view
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    match &dlg.probe_view {
        ProbeView::Report(r) => {
            // Shape invariants, identical to the probe_report pins.
            assert!(r.status.starts_with("pi "), "status: {}", r.status);
            if r.ok {
                assert!(r.binary.is_some(), "ok requires a winning binary");
                assert!(!r.status.contains("(unavailable"), "status: {}", r.status);
            } else {
                assert!(r.status.contains("(unavailable"), "status: {}", r.status);
            }
            // On THIS host (pi provisioned per F-16) the live report is
            // the found-state line the operator would see on open.
            if r.ok {
                println!(
                    "probe-reported live: {} | {}",
                    r.status,
                    r.binary
                        .as_deref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default()
                );
            }
        }
        ProbeView::Pending => unreachable!("loop exits only on a report"),
    }
    let _ = std::fs::remove_dir_all(&root);
}
