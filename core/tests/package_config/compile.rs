// Standard Uses

// Crate Uses
use crate::package_config::TEST_PACKAGE_CONFIG_PATH;

// External Uses
use std::path::Path;
use comline_core::package::config::ir::compiler::Compile;
use comline_core::package::config::ir::interpreter::ProjectInterpreter;


#[test]
// #[ignore]
fn compile_test_package_package_from_config() {
    let result = ProjectInterpreter::from_origin(Path::new(&*TEST_PACKAGE_CONFIG_PATH));

    assert!(result.is_ok(), "Failed to compile package config: {:?}", result.err());
    let compiled = result.unwrap();

    assert_eq!(compiled.config.name.value, "test"); // config.idp has "congregation test"
    // Verify frozen config if possible, or just compilation success
}

#[test]
fn a_dependencies_block_freezes_instead_of_panicking() {
    use comline_core::package::config::ir::frozen::{dependencies, FrozenUnit};

    let source = r#"congregation with_deps
specification_version = 1

dependencies = {
    shared_types = {
        path = "../shared-types"
    }
    acme_lib = {
        version = "1.0.0"
        uri = "https://github.com/acme/std"
        commit = "abc123"
        hash = "blake3:deadbeef"
    }
}
"#;

    let context = ProjectInterpreter::from_config_source(source)
        .expect("a well-formed dependencies block must not panic the interpreter");

    let frozen = comline_core::package::config::ir::interpreter::interpret::interpret_context(
        &context,
    )
    .expect("interpretation should succeed");

    let deps = dependencies(&frozen);
    assert_eq!(deps.len(), 2, "both dependencies should freeze: {:?}", frozen);

    let shared = deps
        .iter()
        .find(|d| d.project == "shared_types")
        .expect("shared_types dependency");
    assert_eq!(shared.author, "local");
    assert_eq!(
        shared.version, "unresolved",
        "a Path dependency has no declared version until resolved"
    );

    let acme_dep = deps.iter().find(|d| d.project == "acme_lib").expect("acme_lib dependency");
    assert_eq!(acme_dep.author, "acme");
    assert_eq!(acme_dep.version, "1.0.0");

    // sanity: make sure we didn't just get an empty freeze
    assert!(frozen.iter().any(|u| matches!(u, FrozenUnit::SpecificationVersion(1))));
}
