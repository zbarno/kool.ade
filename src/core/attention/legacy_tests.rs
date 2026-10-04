use super::*;

#[test]
fn legacy_non_english_alternatives_are_button_choices_only_when_grounded() {
    let report = Report {
        status: ReportStatus::Blocked,
        blocker_disposition: BlockerDisposition::HumanAction,
        summary: "La cuota del servicio está agotada.".into(),
        acceptance_criteria: Vec::new(),
        verification: Vec::new(),
        remaining: vec!["Responsable: elegir un camino: esperar al reinicio mensual; solicitar un aumento temporal de cuota.".into()],
        human_choices: Vec::new(),
    };
    let brief = Brief {
        problem: "El proveedor agotó la cuota diaria, así que el trabajo debe esperar o el responsable puede pedir más capacidad.".into(),
        recommendation: None,
        options: vec![
            OptionBrief {
                id: "wait-for-reset".into(),
                label: "Esperar al reinicio".into(),
                meaning: "Usar la cuota incluida cuando se reinicie.".into(),
                consequence: "El trabajo seguirá detenido hasta entonces.".into(),
                source_evidence: Some("esperar al reinicio mensual".into()),
            },
            OptionBrief {
                id: "request-temporary-increase".into(),
                label: "Pedir más cuota".into(),
                meaning: "Solicitar capacidad temporal al proveedor.".into(),
                consequence: "El proveedor puede tardar o cobrar más.".into(),
                source_evidence: Some("solicitar un aumento temporal de cuota".into()),
            },
        ],
        steps: Vec::new(),
        after: "Kool.ad/e puede continuar cuando haya cuota disponible.".into(),
    };
    assert!(validate(&brief, &report).is_ok());

    let mut unsupported = brief;
    unsupported.options[1].source_evidence = Some("contact technical support".into());
    assert!(validate(&unsupported, &report).is_err());
}

#[test]
fn a_sequence_of_required_steps_is_not_misrepresented_as_user_choices() {
    let report = Report {
        status: ReportStatus::Blocked,
        blocker_disposition: BlockerDisposition::HumanAction,
        summary: "The dashboard session expired before export finished.".into(),
        acceptance_criteria: Vec::new(),
        verification: Vec::new(),
        remaining: vec![
            "Account owner: sign in to the dashboard.".into(),
            "Account owner: export the run log.".into(),
            "Kool.ad/e: resume after the file is saved.".into(),
        ],
        human_choices: Vec::new(),
    };
    let brief = Brief {
        problem: "The dashboard session ended before Kool.ad/e could download the run log.".into(),
        recommendation: None,
        options: Vec::new(),
        steps: vec![HumanStep {
            owner: "Account owner".into(),
            action: "Sign in and export the run log.".into(),
        }],
        after: "Kool.ad/e can resume after the file is saved.".into(),
    };
    assert!(validate(&brief, &report).is_ok());
}
