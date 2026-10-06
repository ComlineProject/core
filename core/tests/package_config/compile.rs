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

#[test]
fn a_settings_block_freezes_into_a_nested_dict() {
    use comline_core::package::config::ir::frozen::settings;
    use comline_core::settings::value::SettingsValue;

    let source = r#"congregation with_settings
specification_version = 1

settings = {
    validators = {
        allowed = true
    }
    max_depth = 8
}
"#;

    let context = ProjectInterpreter::from_config_source(source)
        .expect("a well-formed settings block must not panic the interpreter");
    let frozen = comline_core::package::config::ir::interpreter::interpret::interpret_context(
        &context,
    )
    .expect("interpretation should succeed");

    let dict = settings(&frozen).expect("a Settings unit should be frozen");
    let SettingsValue::Dict(validators) = dict.get("validators").expect("validators key") else {
        panic!("expected a nested dict at 'validators'");
    };
    assert_eq!(validators.get("allowed"), Some(&SettingsValue::Bool(true)));
    assert_eq!(dict.get("max_depth"), Some(&SettingsValue::Integer(8)));
}

#[test]
fn an_absent_settings_key_still_freezes_an_empty_default() {
    use comline_core::package::config::ir::frozen::settings;

    let source = r#"congregation no_settings
specification_version = 1
"#;

    let context = ProjectInterpreter::from_config_source(source)
        .expect("a minimal config must not panic the interpreter");
    let frozen = comline_core::package::config::ir::interpreter::interpret::interpret_context(
        &context,
    )
    .expect("interpretation should succeed");

    let dict = settings(&frozen).expect("a default Settings unit should still be synthesised");
    assert!(dict.is_empty(), "no real default policy content ships yet");
}

#[test]
fn settings_with_mode_replace_bare_keyword_freezes_without_panicking() {
    use comline_core::package::config::ir::frozen::settings;
    use comline_core::settings::value::SettingsValue;

    let source = r#"congregation with_mode
specification_version = 1

settings = {
    overrides = {
        mode = replace
        a = true
    }
}
"#;

    let context = ProjectInterpreter::from_config_source(source)
        .expect("must not panic the interpreter");
    let frozen = comline_core::package::config::ir::interpreter::interpret::interpret_context(
        &context,
    )
    .expect("a valid bare-keyword mode value should freeze fine");

    let dict = settings(&frozen).expect("a Settings unit should be frozen");
    let SettingsValue::Dict(overrides) = dict.get("overrides").expect("overrides key") else {
        panic!("expected a nested dict at 'overrides'");
    };
    // The literal `mode` key is frozen as ordinary content here - it's
    // `settings::merge::merge` that strips it when actually merging, not
    // freezing. Only its *value* is validated eagerly at freeze time.
    assert_eq!(overrides.get("a"), Some(&SettingsValue::Bool(true)));
}

#[test]
#[should_panic(expected = "invalid 'settings'")]
fn settings_with_an_invalid_mode_value_panics_at_freeze_time() {
    let source = r#"congregation bad_mode
specification_version = 1

settings = {
    mode = merge
}
"#;

    let context = ProjectInterpreter::from_config_source(source)
        .expect("must not panic the interpreter");
    comline_core::package::config::ir::interpreter::interpret::interpret_context(&context)
        .expect("should have panicked before returning");
}

#[test]
fn a_changed_settings_unit_does_not_bump_the_version() {
    use comline_core::package::build::VersionBump;
    use comline_core::package::config::ir::diff::analyze::analyze_config_changes;
    use comline_core::package::config::ir::frozen::FrozenUnit;

    let prev = vec![
        FrozenUnit::SpecificationVersion(1),
        FrozenUnit::Settings(Default::default()),
    ];
    let mut cur_dict = comline_core::settings::SettingsDict::default();
    cur_dict
        .0
        .insert("a".to_string(), comline_core::settings::SettingsValue::Bool(true));
    let cur = vec![FrozenUnit::SpecificationVersion(1), FrozenUnit::Settings(cur_dict)];

    let changes = analyze_config_changes(&prev, &cur);
    assert_eq!(changes.bump(), VersionBump::None, "settings changes: {:?}", changes);
}
