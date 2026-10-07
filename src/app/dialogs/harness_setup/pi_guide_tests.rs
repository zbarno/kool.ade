use super::pi_guide::{GuideLine, GuideLineKind as K, PiGuideView, ProbeReport, guide_lines};
use super::*;

fn gl(kind: K, text: &str) -> GuideLine {
    GuideLine {
        kind,
        text: text.into(),
    }
}

fn report(status: &str, diagnostic: &str, binary: Option<&str>, ok: bool) -> ProbeReport {
    ProbeReport {
        status: status.into(),
        diagnostic: diagnostic.into(),
        binary: binary.map(std::path::PathBuf::from),
        ok,
        configuration_required: false,
    }
}

#[test]
fn pi_guide_found_state_preserves_actionable_discovery_and_installation_steps() {
    let view = PiGuideView::Report(report(
        "pi 0.84.4",
        "",
        Some("/home/op/.local/bin/pi"),
        true,
    ));
    let lines = guide_lines(&view, Some("/home/op"));
    let expected = vec![
        gl(K::Title, "Set up the pi harness"),
        gl(
            K::Lead,
            "Kool.ad/e shells out to a locally installed pi CLI; it downloads and installs nothing itself.",
        ),
        gl(K::Status, "pi 0.84.4"),
        gl(K::Detail, "Winning binary: /home/op/.local/bin/pi"),
        gl(K::Rule, "Discovery order — first match wins:"),
        gl(
            K::Order,
            "1. KOOLADE_PI_BIN override: if set and executable it wins outright; a bad value fails fast with no fall-through.",
        ),
        gl(K::Order, "2. pi in every PATH directory, in PATH order."),
        gl(K::Order, "3. /home/op/.npm-global/bin/pi"),
        gl(K::Order, "4. /home/op/.local/bin/pi"),
        gl(K::Order, "5. /home/op/.pi/bin/pi"),
        gl(
            K::Rule,
            "Version policy: no floor, no pinning — any installed pi is accepted; the probed version is display-only (D-13).",
        ),
        gl(K::Lead, "Install & make discoverable:"),
        gl(
            K::Step,
            "1. Obtain pi via the vendor channel — npm install -g @earendil-works/pi-coding-agent (adjust if the vendor's documented channel differs).",
        ),
        gl(
            K::Step,
            "2. Make it reachable via PATH, a home location above, or KOOLADE_PI_BIN=/abs/path/to/pi in the launching environment.",
        ),
        gl(
            K::Step,
            "3. Reopen this dialog and confirm the status reads 'pi <version>'.",
        ),
    ];
    assert_eq!(lines, expected);
}

#[test]
fn pi_guide_unavailable_state_shows_the_diagnostic_and_does_not_claim_success() {
    let lines = guide_lines(
        &PiGuideView::Report(report(
            "pi (unavailable: Pi harness not found)",
            "KOOLADE_PI_BIN=/nonexistent/pi is not an executable file",
            None,
            false,
        )),
        Some("/home/op"),
    );
    assert!(lines.iter().any(|line| {
        line.kind == K::Detail
            && line.text == "KOOLADE_PI_BIN=/nonexistent/pi is not an executable file"
    }));
    assert!(
        !lines
            .iter()
            .any(|line| line.text.contains("Winning binary"))
    );
    assert!(!lines.iter().any(|line| line.text.contains("fallback")));
}

#[test]
fn pending_pi_guide_omits_detail_and_uses_literal_home_when_unset() {
    let lines = guide_lines(&PiGuideView::Pending, None);
    assert!(
        lines
            .iter()
            .any(|line| { line.kind == K::Status && line.text == "Looking for the pi CLI…" })
    );
    assert!(!lines.iter().any(|line| line.kind == K::Detail));
    let tiers = lines
        .iter()
        .filter(|line| line.kind == K::Order)
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert_eq!(tiers.len(), 5);
    assert_eq!(tiers[2], "3. $HOME/.npm-global/bin/pi");
    assert_eq!(tiers[3], "4. $HOME/.local/bin/pi");
    assert_eq!(tiers[4], "5. $HOME/.pi/bin/pi");
}

#[test]
fn pi_guide_tracks_tool_discovery_from_pending_to_live_report() {
    let mut dialog = DlgHarnessSetup {
        settings: crate::persistence::harness_settings::HarnessSettings::default(),
        probe_view: ProbeView::Pending,
        feedback: None,
        section: HarnessSettingsSection::Tools,
        manual_path_drafts: std::collections::BTreeMap::new(),
        probe_rx: None,
    };
    assert!(matches!(
        super::pi_guide::pi_guide_view(&dialog),
        PiGuideView::Pending
    ));
    dialog.settings.discovered.insert(
        "pi".into(),
        crate::persistence::harness_settings::DetectedHarness {
            status: "pi 0.30.1".into(),
            version: Some("0.30.1".into()),
            executable: Some("/example/bin/pi".into()),
            diagnostic: None,
            ready: true,
            models: vec![],
            default_model: None,
            configuration_required: false,
        },
    );
    dialog.probe_view = ProbeView::Complete;
    let view = super::pi_guide::pi_guide_view(&dialog);
    assert!(matches!(view, PiGuideView::Report(_)));
    let lines = guide_lines(&view, Some("/example"));
    assert!(lines.iter().any(|line| line.text == "pi 0.30.1"));
    assert!(
        lines
            .iter()
            .any(|line| line.text == "Winning binary: /example/bin/pi")
    );
}
