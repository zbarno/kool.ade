use super::*;

struct Target {
    id: String,
    surface: String,
    recipes: Vec<String>,
    build: bool,
    test: bool,
    start: bool,
    readiness: bool,
    dependencies: Vec<String>,
    drivers: Vec<String>,
}

impl TargetCapabilityView for Target {
    fn target_id(&self) -> &str {
        &self.id
    }
    fn surface_id(&self) -> &str {
        &self.surface
    }
    fn recipe_ids(&self) -> Vec<&str> {
        self.recipes.iter().map(String::as_str).collect()
    }
    fn has_build_recipe(&self) -> bool {
        self.build
    }
    fn has_test_recipe(&self) -> bool {
        self.test
    }
    fn has_start_recipe(&self) -> bool {
        self.start
    }
    fn has_readiness_probe(&self) -> bool {
        self.readiness
    }
    fn required_dependency_ids(&self) -> &[String] {
        &self.dependencies
    }
    fn supported_driver_ids(&self) -> &[String] {
        &self.drivers
    }
}

#[derive(Default)]
struct Inventory {
    recipes: std::collections::BTreeMap<String, (bool, Vec<String>)>,
    tools: BTreeSet<String>,
    dependencies: BTreeSet<String>,
    drivers: BTreeSet<String>,
}

impl CapabilityInventory for Inventory {
    fn recipe(&self, id: &str) -> Option<RecipeCapabilityView<'_>> {
        self.recipes
            .get(id)
            .map(|(authorized, tools)| RecipeCapabilityView {
                authorized: *authorized,
                prerequisite_tool_ids: tools,
            })
    }
    fn tool_available(&self, id: &str) -> bool {
        self.tools.contains(id)
    }
    fn dependency_available(&self, id: &str) -> bool {
        self.dependencies.contains(id)
    }
    fn driver_available(&self, id: &str) -> bool {
        self.drivers.contains(id)
    }
}

fn target(surface: &str) -> Target {
    Target {
        id: "app".into(),
        surface: surface.into(),
        recipes: vec!["build".into(), "test".into()],
        build: true,
        test: true,
        start: surface == "web" || surface == "linux_desktop",
        readiness: surface == "web" || surface == "linux_desktop",
        dependencies: vec![],
        drivers: vec!["browser".into()],
    }
}

fn ready_inventory() -> Inventory {
    let mut inventory = Inventory::default();
    inventory
        .recipes
        .insert("build".into(), (true, vec!["python".into()]));
    inventory
        .recipes
        .insert("test".into(), (true, vec!["python".into()]));
    inventory.tools.insert("python".into());
    inventory.drivers.insert("browser".into());
    inventory
}

#[test]
fn web_targets_share_driver_resolution_across_language_stacks() {
    let inventory = ready_inventory();
    let projects = [
        ("Python", "FastAPI", target("web")),
        ("TypeScript", "Next.js", target("web")),
    ];
    for (language, framework, target) in projects {
        assert!(!language.is_empty() && !framework.is_empty());
        let assessment = assess_target(&target, &inventory);
        assert!(assessment.gaps.is_empty());
        assert_eq!(assessment.qa_applicability, QaApplicability::Visual);
        assert_eq!(assessment.selected_driver_id.as_deref(), Some("browser"));
    }
}

#[test]
fn headless_service_uses_test_evidence_without_claiming_visual_qa() {
    let target = target("headless_service");
    let assessment = assess_target(&target, &ready_inventory());
    assert_eq!(
        assessment.qa_applicability,
        QaApplicability::ServiceEvidence
    );
    assert_eq!(assessment.selected_driver_id, None);
}

#[test]
fn mobile_target_is_reported_unsupported_instead_of_falling_back_to_web() {
    let target = target("android");
    let assessment = assess_target(&target, &ready_inventory());
    assert_eq!(assessment.qa_applicability, QaApplicability::Unsupported);
    assert!(
        assessment
            .gaps
            .iter()
            .any(|gap| matches!(gap, CapabilityGap::UnsupportedSurface(_)))
    );
}

#[test]
fn unapproved_project_recipe_is_never_reported_ready() {
    let target = target("web");
    let mut inventory = ready_inventory();
    inventory.recipes.insert("build".into(), (false, vec![]));
    let assessment = assess_target(&target, &inventory);
    assert!(
        assessment
            .gaps
            .iter()
            .any(|gap| matches!(gap, CapabilityGap::UnauthorizedRecipe(id) if id == "build"))
    );
}

#[test]
fn missing_project_toolchain_is_actionable_without_a_rust_assumption() {
    let target = target("web");
    let mut inventory = ready_inventory();
    inventory.tools.clear();
    let assessment = assess_target(&target, &inventory);
    assert!(
        assessment
            .gaps
            .contains(&CapabilityGap::MissingToolchain("python".into()))
    );
}

#[test]
fn no_ui_start_or_probe_means_ready_but_untested_not_a_qa_pass() {
    let mut target = target("web");
    target.start = false;
    target.readiness = false;
    let assessment = assess_target(&target, &ready_inventory());
    assert_eq!(
        assessment.qa_applicability,
        QaApplicability::StaticReviewOnly
    );
    assert!(assessment.gaps.contains(&CapabilityGap::MissingStartRecipe));
    assert!(
        assessment
            .gaps
            .contains(&CapabilityGap::MissingReadinessProbe)
    );
}
