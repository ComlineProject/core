//! End-to-end: all three settings layers (package default, schema's own
//! unnamed block, a named preset applied via `@settings`), composed
//! through a real in-memory `PackageSources` package - no filesystem
//! fixture needed.

use comline_core::package::build::PackageSources;
use comline_core::schema::ir::frozen::unit::FrozenUnit as SchemaFrozenUnit;
use comline_core::settings::effective::effective_settings;
use comline_core::settings::value::SettingsValue;

fn b(v: bool) -> SettingsValue {
    SettingsValue::Bool(v)
}

#[test]
fn package_schema_and_declaration_layers_compose() {
    let context = PackageSources::new()
        .config(
            r#"
congregation acme
specification_version = 1
code_generation = { languages = { rust#1.70.0 = {} } }
settings = {
    validators = {
        allowed = true
    }
    untouched = true
}
"#,
        )
        .schema(
            ["chat"],
            r#"
settings {
    validators.allowed = False
}

settings Strict {
    validators.mode = replace
    validators.allowed = True
}

@settings = Strict
struct Message { body: str }
"#,
        )
        .compile()
        .expect("package should compile");

    let schema_context = context
        .schema_contexts
        .iter()
        .find(|s| s.borrow().namespace == vec!["chat".to_string()])
        .expect("the chat schema should be registered")
        .clone();
    let schema_units = schema_context
        .borrow()
        .frozen_schema
        .borrow()
        .clone()
        .expect("the schema should be frozen");

    // Layer 1: package default alone (no schema units in view).
    let package_only = effective_settings(&context, &[], None);
    let SettingsValue::Dict(validators) = package_only.get("validators").unwrap() else {
        panic!("expected a dict");
    };
    assert_eq!(validators.get("allowed"), Some(&b(true)));
    assert_eq!(package_only.get("untouched"), Some(&b(true)));

    // Layer 2: schema-effective (package default + chat's own unnamed
    // block). The schema's dotted-key override flips `validators.allowed`
    // to false; `untouched` still comes through from the package default
    // since the schema never mentions it.
    let schema_effective = effective_settings(&context, &schema_units, None);
    let SettingsValue::Dict(validators) = schema_effective.get("validators").unwrap() else {
        panic!("expected a dict");
    };
    assert_eq!(validators.get("allowed"), Some(&b(false)));
    assert_eq!(schema_effective.get("untouched"), Some(&b(true)));

    // Layer 3: declaration-effective, for `Message`, with `@settings =
    // Strict` applied. `Strict`'s own `mode = replace` means the result
    // at `validators` is `Strict`'s content verbatim, not a merge with
    // the schema-effective layer beneath it - proving the replace
    // short-circuit survives all the way through the three-layer
    // composition.
    let message_parameters = schema_units
        .iter()
        .find_map(|u| match u {
            SchemaFrozenUnit::Struct { name, parameters, .. } if name == "Message" => {
                Some(parameters.clone())
            }
            _ => None,
        })
        .expect("Message struct should be frozen");

    let declaration_effective =
        effective_settings(&context, &schema_units, Some(&message_parameters));
    let SettingsValue::Dict(validators) = declaration_effective.get("validators").unwrap() else {
        panic!("expected a dict");
    };
    assert_eq!(
        validators.get("allowed"),
        Some(&b(true)),
        "Strict's mode=replace wins outright over the schema-effective layer"
    );
    // `untouched` came from the package default, which the schema's
    // unnamed block never overwrote - but Strict's `mode = replace` only
    // replaces the `validators` key's own subtree (where it's written),
    // not the whole top-level dict, so `untouched` still survives.
    assert_eq!(declaration_effective.get("untouched"), Some(&b(true)));
}
