//! `@provider` / `@consumer`: which side implements a protocol, and the
//! framing calls the server starts need.

use comline_core::package::build::PackageSources;
use comline_core::schema::ir::frozen::unit::{ConfigValue, FrozenUnit};

const ALLOW_FRAMING: &str = "settings = { annotations = { framing = { allowed = true } } }\n";

fn config(extra: &str) -> String {
    format!(
        "congregation app\nspecification_version = 1\n\
         code_generation = {{ languages = {{ rust#1.70.0 = {{}} }} }}\n{extra}"
    )
}

fn compile(config: &str, schema: &str) -> eyre::Result<Vec<FrozenUnit>> {
    let ctx = PackageSources::new().config(config).schema(["app"], schema).compile()?;
    let schema = ctx.schema_contexts[0].borrow();
    let units = schema.frozen_schema.borrow().clone().unwrap();
    Ok(units)
}

fn protocol(annotations: &str) -> String {
    format!("{annotations}protocol Events {{\n    function message(text: string) -> bool;\n}}\n")
}

#[test]
fn a_client_provided_protocol_over_datagram2_is_frozen_with_its_side() {
    let units = compile(
        &config(ALLOW_FRAMING),
        &protocol("@provider = Client\n@framing = \"datagram@2\"\n"),
    )
    .unwrap();
    let parameters = units
        .iter()
        .find_map(|u| match u {
            FrozenUnit::Protocol { parameters, .. } => Some(parameters),
            _ => None,
        })
        .unwrap();
    assert!(parameters.contains(&FrozenUnit::Property {
        name: "provider".to_string(),
        value: ConfigValue::Name("Client".to_string()),
    }));
}

#[test]
fn jsonrpc_carries_calls_both_ways_too() {
    compile(&config(ALLOW_FRAMING), &protocol("@provider = Multiple\n@framing = \"jsonrpc\"\n")).unwrap();
}

#[test]
fn a_server_provided_protocol_needs_nothing() {
    compile(&config(""), &protocol("@provider = Server\n")).unwrap();
}

#[test]
fn a_client_provided_protocol_is_on_a_connection_by_default() {
    // No `@framing` means `datagram@2`, which marks requests and replies.
    compile(&config(""), &protocol("@provider = Client\n")).unwrap();
}

#[test]
fn calls_the_server_starts_need_a_framing_that_marks_them() {
    for (config, annotations) in [
        (config(ALLOW_FRAMING), "@provider = Multiple\n@framing = \"datagram\"\n"),
        (config(ALLOW_FRAMING), "@provider = Client\n@framing = \"datagram@1\"\n"),
        // The package's framing reaches the protocol like its own would.
        (config("wire = { framing = \"datagram@1\" }\n"), "@provider = Client\n"),
    ] {
        let err = compile(&config, &protocol(annotations)).unwrap_err().to_string();
        assert!(err.contains("validation::reverse-calls-need-framing"), "{annotations}: {err}");
        assert!(err.contains("datagram@2"), "{err}");
    }
}

#[test]
fn provider_and_consumer_have_to_agree() {
    let err = compile(&config(""), &protocol("@provider = Server\n@consumer = Server\n"))
        .unwrap_err()
        .to_string();
    assert!(err.contains("validation::provider-consumer-mismatch"), "{err}");
    assert!(err.contains("`@consumer = Client`"), "{err}");

    compile(&config(""), &protocol("@provider = Server\n@consumer = Client\n")).unwrap();
}

#[test]
fn only_the_three_sides_are_accepted() {
    let err = compile(&config(""), &protocol("@provider = Browser\n")).unwrap_err().to_string();
    assert!(err.contains("'@provider' must be"), "{err}");
    let err = compile(&config(""), &protocol("@provider = \"Client\"\n")).unwrap_err().to_string();
    assert!(err.contains("'@provider' must be"), "{err}");
}

#[test]
fn a_framing_can_be_a_dictionary_naming_it() {
    let units = compile(
        &config(ALLOW_FRAMING),
        &protocol("@provider = Client\n@framing = { name = \"jsonrpc\", method_names = qualified }\n"),
    )
    .unwrap();
    let parameters = units
        .iter()
        .find_map(|u| match u {
            FrozenUnit::Protocol { parameters, .. } => Some(parameters),
            _ => None,
        })
        .unwrap();
    assert!(parameters.contains(&FrozenUnit::Property {
        name: "framing".to_string(),
        value: ConfigValue::Dict(vec![
            ("name".to_string(), ConfigValue::Str("jsonrpc".to_string())),
            ("method_names".to_string(), ConfigValue::Name("qualified".to_string())),
        ]),
    }));

    // `method_names` as a string works too, and so does the dictionary for
    // the check that calls the server starts need a framing that marks them.
    compile(
        &config(ALLOW_FRAMING),
        &protocol("@provider = Multiple\n@framing = { name = \"datagram@2\" }\n"),
    )
    .unwrap();
    compile(
        &config(ALLOW_FRAMING),
        &protocol("@provider = Client\n@framing = { name = \"jsonrpc\", method_names = \"bare\" }\n"),
    )
    .unwrap();
    let err = compile(&config(ALLOW_FRAMING), &protocol("@provider = Client\n@framing = { name = \"datagram\" }\n"))
        .unwrap_err()
        .to_string();
    assert!(err.contains("validation::reverse-calls-need-framing"), "{err}");
}

#[test]
fn a_framing_dictionary_is_checked_key_by_key() {
    for (framing, expected) in [
        ("{ method_names = qualified }", "`name` is missing"),
        ("{ name = \"jsonrpc\", method_names = loud }", "'method_names' must be"),
        ("{ name = \"jsonrpc\", colour = \"red\" }", "there's no key `colour`"),
        ("{ name = \"carrier-pigeon\" }", "'name' must be one of"),
        ("{ name = \"datagram@2\", method_names = qualified }", "`method_names` applies to JSON-RPC, not `datagram@2`"),
    ] {
        let err = compile(&config(ALLOW_FRAMING), &protocol(&format!("@framing = {framing}\n")))
            .unwrap_err()
            .to_string();
        assert!(err.contains("validation::annotation-value-mismatch"), "{framing}: {err}");
        assert!(err.contains(expected), "{framing}: {err}");
    }
}
