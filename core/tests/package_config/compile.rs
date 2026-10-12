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
        &context, None,
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
        &context, None,
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
fn an_absent_settings_key_still_freezes_the_built_in_default() {
    use comline_core::package::config::ir::frozen::settings;
    use comline_core::settings::value::SettingsValue;

    let source = r#"congregation no_settings
specification_version = 1
"#;

    let context = ProjectInterpreter::from_config_source(source)
        .expect("a minimal config must not panic the interpreter");
    let frozen = comline_core::package::config::ir::interpreter::interpret::interpret_context(
        &context, None,
    )
    .expect("interpretation should succeed");

    let dict = settings(&frozen).expect("a default Settings unit should still be synthesised");
    assert_eq!(
        dict.get_path("annotations.framing.allowed"),
        Some(&SettingsValue::Bool(false)),
        "framing is forbidden by Comline's built-in default"
    );
}

#[test]
fn a_written_settings_block_still_gets_the_built_in_default_underneath() {
    use comline_core::package::config::ir::frozen::settings;
    use comline_core::settings::value::SettingsValue;

    // Regression: the built-in default used to be installed only when a
    // package wrote no `settings` key at all, so writing any unrelated
    // setting silently dropped the framing protection entirely.
    let source = r#"congregation writes_unrelated_settings
specification_version = 1

settings = {
    validators = {
        allowed = true
    }
}
"#;

    let context = ProjectInterpreter::from_config_source(source).expect("should parse");
    let frozen = comline_core::package::config::ir::interpreter::interpret::interpret_context(
        &context, None,
    )
    .expect("interpretation should succeed");

    let dict = settings(&frozen).expect("a Settings unit should be frozen");
    assert_eq!(
        dict.get_path("annotations.framing.allowed"),
        Some(&SettingsValue::Bool(false)),
        "the built-in default must survive a package writing its own unrelated settings"
    );
    assert_eq!(
        dict.get_path("validators.allowed"),
        Some(&SettingsValue::Bool(true)),
        "the package's own setting must still be there"
    );
}

#[test]
fn a_misspelled_settings_key_in_idp_is_rejected_not_panicked() {
    // The `.idp` path now runs the same registry check as `.ids`, and
    // reports it as an error rather than panicking.
    let source = r#"congregation bad_key
specification_version = 1

settings = {
    annotations = {
        framing = {
            alowed = true
        }
    }
}
"#;

    let context = ProjectInterpreter::from_config_source(source).expect("should parse");
    let err = comline_core::package::config::ir::interpreter::interpret::interpret_context(
        &context, None,
    )
    .expect_err("an unrecognised key in a package manifest is an error");

    let text = err.to_string();
    assert!(text.contains("settings::unknown-key"), "got: {text}");
    assert!(text.contains("alowed"), "names the offending key: {text}");
}

#[test]
fn an_open_namespace_key_in_idp_still_freezes() {
    // `.idp` settings stay as open at the top level as `.ids` ones.
    let source = r#"congregation open_key
specification_version = 1

settings = {
    max_depth = 8
    overrides = {
        mode = replace
    }
}
"#;

    let context = ProjectInterpreter::from_config_source(source).expect("should parse");
    comline_core::package::config::ir::interpreter::interpret::interpret_context(&context, None)
        .expect("an open-namespace key must not fail package interpretation");
}

#[test]
fn a_package_can_opt_out_of_the_built_in_default_with_mode_replace() {
    use comline_core::package::config::ir::frozen::settings;

    // The escape hatch for the merge above, which `merge` already
    // implements: `mode = replace` at the root drops the layer below.
    let source = r#"congregation opts_out
specification_version = 1

settings = {
    mode = replace
}
"#;

    let context = ProjectInterpreter::from_config_source(source).expect("should parse");
    let frozen = comline_core::package::config::ir::interpreter::interpret::interpret_context(
        &context, None,
    )
    .expect("interpretation should succeed");

    let dict = settings(&frozen).expect("a Settings unit should be frozen");
    assert_eq!(
        dict.get_path("annotations.framing.allowed"),
        None,
        "`mode = replace` at the root drops the built-in layer"
    );
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
        &context, None,
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
    comline_core::package::config::ir::interpreter::interpret::interpret_context(&context, None)
        .expect("should have panicked before returning");
}

#[test]
fn a_bad_manifest_is_an_error_rather_than_a_panic() {
    // Every one of these used to `panic!`, which in the language server
    // meant the whole process died the moment a schema in the package was
    // opened - the editor just lost its connection, with no diagnostic
    // and nothing pointing at the manifest. A build is only marginally
    // better off: a panic and a backtrace instead of an error.
    let cases = [
        // The one that actually bit: `std` is embedded, so it cannot be
        // declared as a dependency.
        ("dependencies = {\n    std = {}\n}\n", "reserved"),
        ("specification_version = \"one\"\n", "specification_version"),
        ("dependencies = 3\n", "dependencies"),
        ("code_generation = 3\n", "code_generation"),
        ("code_generation = {\n    languages = 3\n}\n", "languages"),
        ("settings = 3\n", "settings"),
        ("not_a_real_key = 3\n", "not a recognised"),
    ];

    for (fragment, expected) in cases {
        let source = format!("congregation bad\n{fragment}");
        let context = ProjectInterpreter::from_config_source(&source)
            .unwrap_or_else(|e| panic!("{fragment:?} should still parse: {e}"));

        let err = comline_core::package::config::ir::interpreter::interpret::interpret_context(
            &context, None,
        )
        .err()
        .unwrap_or_else(|| panic!("{fragment:?} should be an error, not accepted"));

        let text = err.to_string();
        assert!(
            text.contains(expected),
            "{fragment:?}: expected {expected:?} in {text:?}"
        );
    }
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
