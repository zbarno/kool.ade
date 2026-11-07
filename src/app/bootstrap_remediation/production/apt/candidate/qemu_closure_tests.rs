use super::{check_simulated_dependency_closure, parse_install_plan};
use crate::app::setup_attention::catalog::LinuxRootlessRecipeV1 as Recipe;

#[test]
fn simulation_plan_captures_installed_and_upgraded_versions() {
    let output = b"NOTE: This is only a simulation!\nInst libfoo:amd64 (1:2.3-1 Debian:13/stable [amd64])\nInst qemu-utils [1:9.2.0-1] (1:9.2.1-2 Debian:13/stable [amd64])\nConf qemu-utils (1:9.2.1-2 Debian:13/stable [amd64])\n";
    assert_eq!(
        parse_install_plan(output).unwrap(),
        [
            ("libfoo:amd64".into(), "1:2.3-1".into()),
            ("qemu-utils".into(), "1:9.2.1-2".into()),
        ]
    );
}

#[test]
fn simulation_plan_rejects_removals_duplicate_packages_and_malformed_inst_lines() {
    for output in [
        b"Remv unrelated [1.0]\n".as_slice(),
        b"Inst qemu-utils (1.0 Debian:13/stable [amd64])\nInst qemu-utils (1.0 Debian:13/stable [amd64])\n",
        b"Inst qemu-utils unavailable\n",
    ] {
        assert!(parse_install_plan(output).is_err());
    }
}

#[test]
fn simulation_plan_rejects_invalid_encoding_and_oversized_or_hostile_tokens() {
    assert!(parse_install_plan(&[0xff]).is_err());
    assert!(parse_install_plan(b"Inst qemu-utils;evil (1.0 Debian:13/stable [amd64])\n").is_err());
    assert!(parse_install_plan(b"Inst qemu-utils (1.0;evil Debian:13/stable [amd64])\n").is_err());
    let oversized = vec![b' '; super::MAX_PLAN_BYTES + 1];
    assert!(parse_install_plan(&oversized).is_err());
}

#[test]
fn simulated_dependency_closure_checks_each_selected_origin_and_rejects_foreign_packages() {
    // This tests the production closure-validation seam with a fake apt-cache lookup. It proves
    // that every selected dependency reaches origin validation before package installation.
    let output =
        b"Inst libgood (1.0 Debian:13/stable [amd64])\nInst libforeign (2.0 vendor/repo [amd64])\n";
    let mut checked = Vec::new();
    let result = check_simulated_dependency_closure(
        output,
        Recipe::Debian13Amd64,
        |package, version, recipe| {
            checked.push((package.to_owned(), version.to_owned(), recipe));
            if package == "libforeign" {
                anyhow::bail!("synthetic foreign origin")
            }
            Ok(())
        },
    );

    assert!(result.is_err());
    assert_eq!(checked.len(), 2);
    assert_eq!(checked[0].0, "libgood");
    assert_eq!(checked[1].0, "libforeign");
    assert!(result.unwrap_err().to_string().contains("libforeign 2.0"));
}
