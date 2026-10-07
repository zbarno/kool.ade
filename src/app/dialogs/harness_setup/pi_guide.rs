use super::{DlgHarnessSetup, ProbeView};

pub type ProbeReport = crate::harness::pi_harness::ProbeReport;

#[derive(Clone, Default, Debug, PartialEq)]
pub enum PiGuideView {
    #[default]
    Pending,
    Report(ProbeReport),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GuideLineKind {
    Title,
    Lead,
    Status,
    Detail,
    Order,
    Rule,
    Step,
}

#[derive(Debug, PartialEq)]
pub struct GuideLine {
    pub kind: GuideLineKind,
    pub text: String,
}

pub fn pi_guide_view(dialog: &DlgHarnessSetup) -> PiGuideView {
    if matches!(dialog.probe_view, ProbeView::Pending) {
        return PiGuideView::Pending;
    }
    let Some(detected) = dialog.settings.discovered.get("pi") else {
        return PiGuideView::Report(ProbeReport {
            status: "pi (unavailable: discovery did not return Pi)".into(),
            diagnostic: "Rediscover the supported coding tools and check the reported status."
                .into(),
            binary: None,
            ok: false,
            configuration_required: false,
        });
    };
    PiGuideView::Report(ProbeReport {
        status: detected.status.clone(),
        diagnostic: detected.diagnostic.clone().unwrap_or_default(),
        binary: detected.executable.as_deref().map(Into::into),
        ok: detected.ready,
        configuration_required: detected.configuration_required,
    })
}

pub fn guide_lines(view: &PiGuideView, home: Option<&str>) -> Vec<GuideLine> {
    use GuideLineKind as K;
    let env = crate::harness::pi_harness::PI_BINARY_ENV;
    let home_display = home.unwrap_or("$HOME");
    let mut lines = vec![
        GuideLine {
            kind: K::Title,
            text: "Set up the pi harness".into(),
        },
        GuideLine {
            kind: K::Lead,
            text: "Kool.ad/e shells out to a locally installed pi CLI; it downloads and installs nothing itself.".into(),
        },
        GuideLine {
            kind: K::Status,
            text: match view {
                PiGuideView::Pending => "Looking for the pi CLI…".into(),
                PiGuideView::Report(report) => report.status.clone(),
            },
        },
    ];
    if let PiGuideView::Report(report) = view {
        lines.push(GuideLine {
            kind: K::Detail,
            text: if report.ok {
                format!(
                    "Winning binary: {}",
                    report
                        .binary
                        .as_deref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_default()
                )
            } else {
                report.diagnostic.clone()
            },
        });
    }
    lines.extend([
        GuideLine {
            kind: K::Rule,
            text: "Discovery order — first match wins:".into(),
        },
        GuideLine {
            kind: K::Order,
            text: format!("1. {env} override: if set and executable it wins outright; a bad value fails fast with no fall-through."),
        },
        GuideLine {
            kind: K::Order,
            text: "2. pi in every PATH directory, in PATH order.".into(),
        },
    ]);
    lines.extend(
        crate::harness::pi_harness::COMMON_HOME_SITES
            .iter()
            .enumerate()
            .map(|(index, site)| GuideLine {
                kind: K::Order,
                text: format!("{}. {home_display}/{site}/pi", index + 3),
            }),
    );
    lines.extend([
        GuideLine {
            kind: K::Rule,
            text: "Version policy: no floor, no pinning — any installed pi is accepted; the probed version is display-only (D-13).".into(),
        },
        GuideLine {
            kind: K::Lead,
            text: "Install & make discoverable:".into(),
        },
        GuideLine {
            kind: K::Step,
            text: "1. Obtain pi via the vendor channel — npm install -g @earendil-works/pi-coding-agent (adjust if the vendor's documented channel differs).".into(),
        },
        GuideLine {
            kind: K::Step,
            text: format!("2. Make it reachable via PATH, a home location above, or {env}=/abs/path/to/pi in the launching environment."),
        },
        GuideLine {
            kind: K::Step,
            text: "3. Reopen this dialog and confirm the status reads 'pi <version>'.".into(),
        },
    ]);
    lines
}

pub fn paint(ui: &mut egui::Ui, dialog: &DlgHarnessSetup) {
    let home_raw = std::env::var("HOME").ok();
    let lines = guide_lines(&pi_guide_view(dialog), home_raw.as_deref());
    ui.collapsing("Set up the Pi CLI", |ui| {
        for line in lines {
            let text = match line.kind {
                GuideLineKind::Title => egui::RichText::new(line.text).strong(),
                GuideLineKind::Status => egui::RichText::new(line.text).strong(),
                GuideLineKind::Order | GuideLineKind::Step => {
                    egui::RichText::new(line.text).monospace().size(11.0)
                }
                _ => egui::RichText::new(line.text).size(11.0).weak(),
            };
            ui.label(text);
        }
    });
}
