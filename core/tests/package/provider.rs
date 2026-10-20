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
fn calls_the_server_starts_need_a_framing_that_marks_them() {
    for annotations in [
        "@provider = Client\n",
        "@provider = Multiple\n@framing = \"datagram\"\n",
    ] {
        let config = config(ALLOW_FRAMING);
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
