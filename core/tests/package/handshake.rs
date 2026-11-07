//! The handshake a protocol runs: its own `@handshake`, else the package's
//! `handshake = "..."`, else `comline@1`. The package default is frozen into
//! each protocol it reaches.

use comline_core::package::build::PackageSources;
use comline_core::schema::ir::frozen::unit::{ConfigValue, FrozenUnit};

const SCHEMA: &str = "@unverified = \"talks to a legacy peer\"\nprotocol Plain {\n    function f();\n}\n\n\
                      @handshake = \"comline@1\"\nprotocol Own {\n    function g();\n}\n\n\
                      struct S {\n    a: u32\n}\n";

fn config(handshake: Option<&str>) -> String {
    let line = handshake.map(|h| format!("handshake = \"{h}\"\n")).unwrap_or_default();
    format!(
        "congregation app\nspecification_version = 1\n{line}\n\
         code_generation = {{\n    languages = {{\n        rust#1.70.0 = {{}}\n    }}\n}}\n\
         settings = {{ annotations = {{ unverified = {{ allowed = true }} }} }}\n"
    )
}

fn compile(handshake: Option<&str>, schema: &str) -> eyre::Result<Vec<FrozenUnit>> {
    let ctx = PackageSources::new().config(config(handshake)).schema(["app"], schema).compile()?;
    let schema = ctx.schema_contexts[0].borrow();
    let units = schema.frozen_schema.borrow().clone().unwrap();
    Ok(units)
}

/// A protocol's `@handshake`, as frozen.
fn handshake_of(units: &[FrozenUnit], protocol: &str) -> Option<String> {
    units.iter().find_map(|u| match u {
        FrozenUnit::Protocol { name, parameters, .. } if name == protocol => {
            Some(parameters.iter().find_map(|p| match p {
                FrozenUnit::Property { name, value: ConfigValue::Str(v) } if name == "handshake" => {
                    Some(v.clone())
                }
                _ => None,
            }))
        }
        _ => None,
    })?
}

#[test]
fn the_package_default_is_frozen_into_protocols_without_their_own() {
    let units = compile(Some("none"), SCHEMA).unwrap();
    assert_eq!(handshake_of(&units, "Plain").as_deref(), Some("none"));
    // A protocol's own annotation wins.
    assert_eq!(handshake_of(&units, "Own").as_deref(), Some("comline@1"));
    // A struct takes nothing.
    let s = units.iter().find(|u| matches!(u, FrozenUnit::Struct { name, .. } if name == "S"));
    assert!(matches!(s, Some(FrozenUnit::Struct { parameters, .. }) if parameters.is_empty()));
}

#[test]
fn comline_at_1_stays_implicit_so_ir_is_unchanged() {
    // What an absent `@handshake` has always meant: stating it keeps the IR
    // a package taking the old default had.
    let explicit = compile(Some("comline@1"), SCHEMA).unwrap();
    assert_eq!(handshake_of(&explicit, "Plain"), None);
    // Core's default now is `comline@2`, frozen in like a package's.
    let absent = compile(None, SCHEMA).unwrap();
    assert_eq!(handshake_of(&absent, "Plain").as_deref(), Some("comline@2"));
    assert_ne!(absent, explicit);
}

#[test]
fn an_unknown_package_handshake_is_refused() {
    let typo = compile(Some("comline"), SCHEMA).unwrap_err().to_string();
    assert!(typo.contains("handshake `comline` isn't known"), "{typo}");

    // A namespaced one has to be declared, by the package or a dependency.
    let custom = compile(Some("com.acme.hello@1"), SCHEMA).unwrap_err().to_string();
    assert!(custom.contains("handshake `com.acme.hello@1` isn't declared"), "{custom}");
}

#[test]
fn an_unknown_handshake_annotation_is_refused() {
    let schema = "@handshake = \"com.acme.hello@1\"\nprotocol P {\n    function f();\n}\n";
    let err = format!("{:?}", compile(None, schema).unwrap_err());
    assert!(err.contains("handshake `com.acme.hello@1` isn't declared"), "{err}");
}

// ---------- parameters ----------

/// A protocol's `@handshake`, as frozen, in either form.
fn value_of(units: &[FrozenUnit], protocol: &str) -> Option<ConfigValue> {
    units.iter().find_map(|u| match u {
        FrozenUnit::Protocol { name, parameters, .. } if name == protocol => parameters.iter().find_map(|p| match p {
            FrozenUnit::Property { name, value } if name == "handshake" => Some(value.clone()),
            _ => None,
        }),
        _ => None,
    })
}

/// A package with `handshake = <written>`, and `schema`.
fn compile_written(written: &str, schema: &str) -> eyre::Result<Vec<FrozenUnit>> {
    let config = format!(
        "congregation app\nspecification_version = 1\nhandshake = {written}\n\
         settings = {{ annotations = {{ unverified = {{ allowed = true }} }} }}\n"
    );
    let ctx = PackageSources::new().config(config).schema(["app"], schema).compile()?;
    let schema = ctx.schema_contexts[0].borrow();
    let units = schema.frozen_schema.borrow().clone().unwrap();
    Ok(units)
}

const ONE: &str = "protocol P {\n    function f();\n}\n";

#[test]
fn the_official_package_declares_comline_at_2_and_datagram_at_3() {
    let units = compile(Some("comline@2"), ONE).unwrap();
    assert_eq!(handshake_of(&units, "P").as_deref(), Some("comline@2"));
    compile(None, "@handshake = \"comline@2\"\nprotocol P {\n    function f();\n}\n").unwrap();
    PackageSources::new()
        .config("congregation app\nspecification_version = 1\nwire = { framing = \"datagram@3\" }\n".to_string())
        .schema(["app"], ONE)
        .compile()
        .unwrap();
    // Their neighbours stay typos.
    for typo in ["comline@3", "comline@0"] {
        let err = compile(Some(typo), ONE).unwrap_err().to_string();
        assert!(err.contains(&format!("handshake `{typo}` isn't known")), "{err}");
    }
}

#[test]
fn the_dictionary_form_states_parameters_in_config_and_annotation() {
    use comline_core::schema::idl::annotations::HandshakeValue;

    // Strings and bare words, as `@framing`'s `method_names` takes.
    let units = compile_written("{ name = \"comline@2\" scope = \"package\" exchange = \"symmetric\" }", ONE).unwrap();
    let value = HandshakeValue::of(&value_of(&units, "P").unwrap()).unwrap();
    assert_eq!(value.name, "comline@2");
    assert_eq!(value.param("scope"), Some("package"));
    assert_eq!(value.param("exchange"), Some("symmetric"));
    assert_eq!(value.param("newer_consumer"), Some("degrade"));
    assert_eq!(value.param("nope"), None);

    let schema = "@handshake = { name = \"comline@2\", newer_consumer = refuse, scope = \"package\" }\nprotocol P {\n    function f();\n}\n";
    let units = compile(None, schema).unwrap();
    let value = HandshakeValue::of(&value_of(&units, "P").unwrap()).unwrap();
    assert_eq!(value.param("newer_consumer"), Some("refuse"));
    assert_eq!(value.param("scope"), Some("package"));

    // A protocol's own wins over the package's, whole.
    let schema = "@handshake = { name = \"comline@2\", exchange = symmetric }\nprotocol Own {\n    function f();\n}\n\nprotocol P {\n    function f();\n}\n";
    let units = compile_written("{ name = \"comline@2\" scope = \"package\" }", schema).unwrap();
    let own = HandshakeValue::of(&value_of(&units, "Own").unwrap()).unwrap();
    assert_eq!((own.param("exchange"), own.param("scope")), (Some("symmetric"), Some("protocol")));
    let inherited = HandshakeValue::of(&value_of(&units, "P").unwrap()).unwrap();
    assert_eq!((inherited.param("exchange"), inherited.param("scope")), (Some("offer"), Some("package")));
}

/// The plain form keeps its IR, and the dictionary form with only a name is it.
#[test]
fn a_name_alone_freezes_as_the_plain_form_does() {
    for name in ["comline@1", "comline@2", "none"] {
        let schema = "@unverified = \"a legacy peer\"\nprotocol P {\n    function f();\n}\n";
        let schema = if name == "none" { schema } else { ONE };
        let plain = compile_written(&format!("\"{name}\""), schema).unwrap();
        let dict = compile_written(&format!("{{ name = \"{name}\" }}"), schema).unwrap();
        assert_eq!(plain, dict, "{name}");
    }
    let absent = compile(None, ONE).unwrap();
    assert_eq!(value_of(&absent, "P"), Some(ConfigValue::Str("comline@2".into())));
}

#[test]
fn unknown_parameters_and_values_are_refused() {
    let cases = [
        ("{ name = \"comline@2\" speed = \"fast\" }", "`speed` isn't a parameter of handshake `comline@2`: expected `scope`, `newer_consumer`, `same_version_differs`, `exchange`"),
        ("{ name = \"comline@2\" scope = \"galaxy\" }", "handshake `comline@2`'s `scope` can't be `galaxy`: expected `protocol` or `package`"),
        ("{ name = \"comline@1\" scope = \"package\" }", "handshake `comline@1` takes no parameters, so `scope` can't be set"),
        ("{ name = \"none\" exchange = \"offer\" }", "handshake `none` takes no parameters, so `exchange` can't be set"),
    ];
    for (written, expected) in cases {
        // The same sentence from `config.idp`'s `handshake` and `@handshake`.
        let err = format!("{:?}", compile_written(written, ONE).unwrap_err());
        assert!(err.contains(expected), "{written}: {err}");
        let written = written.replacen("\" ", "\", ", 1);
        let schema = format!("@handshake = {written}\nprotocol P {{\n    function f();\n}}\n");
        let err = format!("{:?}", compile(None, &schema).unwrap_err());
        assert!(err.contains(expected), "{written}: {err}");
    }
    let err = format!("{:?}", compile_written("{ scope = \"package\" }", ONE).unwrap_err());
    assert!(err.contains("'handshake' needs a `name`"), "{err}");
    let err = format!("{:?}", compile_written("{ name = \"comline@2\" scope = \"package\" scope = \"protocol\" }", ONE).unwrap_err());
    assert!(err.contains("`scope` is set twice"), "{err}");
    let err = format!("{:?}", compile_written("7", ONE).unwrap_err());
    assert!(err.contains("'handshake' should be a string"), "{err}");
    let schema = "@handshake = { name = \"comline@9\" }\nprotocol P {\n    function f();\n}\n";
    let err = format!("{:?}", compile(None, schema).unwrap_err());
    assert!(err.contains("handshake `comline@9` isn't known"), "{err}");
}

/// A third party's profile can't declare parameters, as it can't for a framing.
#[test]
fn a_declared_handshake_takes_no_parameters() {
    use comline_core::schema::idl::annotations::check_handshake_params;
    let err = check_handshake_params("com.acme.hello@1", [("depth", "1")]).unwrap_err();
    assert!(err.contains("handshake `com.acme.hello@1` takes no parameters"), "{err}");
}

/// `same_version_differs = warn` gives up version honesty, so it needs the
/// marker, and freezes the guarantee beside it.
#[test]
fn warning_on_a_same_version_difference_gives_up_version_honesty() {
    use comline_core::schema::ir::frozen::unit::Guarantee;
    use comline_core::schema::ir::unverified;

    let written = "{ name = \"comline@2\" same_version_differs = \"warn\" }";
    let err = format!("{:?}", compile_written(written, ONE).unwrap_err());
    assert!(err.contains("gives up version honesty"), "{err}");
    assert!(err.contains("`same_version_differs = warn`"), "{err}");

    let marked = "@unverified = \"rolling out a schema fix\"\nprotocol P {\n    function f();\n}\n";
    let units = compile_written(written, marked).unwrap();
    assert_eq!(unverified::protocols(&units)[0].1, [Guarantee::VersionHonesty]);

    // The other policies give up nothing.
    compile_written("{ name = \"comline@2\" same_version_differs = \"refuse\" newer_consumer = \"refuse\" scope = \"package\" }", ONE).unwrap();
}
