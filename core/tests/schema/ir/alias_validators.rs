// Validators and `@deprecated` on a `type` alias (#99): stored once on the
// alias, checked once there, and applying wherever the alias is used.

use comline_core::schema::ir::compiler::interpreter::incremental::IncrementalInterpreter;
use comline_core::schema::ir::compiler::Compile;
use comline_core::schema::ir::frozen::unit::{ConfigValue, FrozenUnit};
use comline_core::schema::ir::validation::validate;

const BOUNDS: &str = "validator Bounds for string {\n    max: u32 = 10\n    validate {\n        \
                      assert(value.length <= params.max, \"{value.name} is too long\")\n    }\n}\n";

fn messages(code: &str) -> Vec<String> {
    validate(&IncrementalInterpreter::from_source(code))
        .err()
        .unwrap_or_default()
        .into_iter()
        .map(|e| e.message)
        .collect()
}

#[test]
fn an_alias_freezes_its_validators() {
    let units = IncrementalInterpreter::from_source(&format!(
        "{BOUNDS}@validators = [Bounds(max = 5)]\ntype AccountName = string\n"
    ));
    let parameters = units
        .iter()
        .find_map(|u| match u {
            FrozenUnit::TypeAlias { name, parameters, .. } if name == "AccountName" => {
                Some(parameters)
            }
            _ => None,
        })
        .expect("the alias");
    assert_eq!(
        *parameters,
        [FrozenUnit::ValidatorRef {
            name: "Bounds".to_string(),
            args: vec![FrozenUnit::Property {
                name: "max".to_string(),
                value: ConfigValue::Integer(5),
            }],
        }]
    );
    assert!(validate(&units).is_ok());
}

#[test]
fn an_alias_validator_is_checked_against_the_alias_target() {
    let errors = messages(&format!("{BOUNDS}@validators = [Bounds()]\ntype Count = u32\n"));
    assert_eq!(errors, ["validator 'Bounds' applies to `string`, not `u32`"]);
}

#[test]
fn an_alias_of_an_alias_is_checked_against_what_it_resolves_to() {
    let code = format!(
        "{BOUNDS}type Text = string\n@validators = [Bounds()]\ntype Name = Text\n"
    );
    assert!(messages(&code).is_empty());
}

#[test]
fn an_alias_validator_gets_the_same_argument_checks_as_a_field() {
    let errors = messages(&format!(
        "{BOUNDS}@validators = [Bounds(maxx = 1), Nope()]\ntype Name = string\n"
    ));
    assert!(
        errors.iter().any(|m| m.contains("validator 'Bounds' has no argument 'maxx'")),
        "{errors:?}"
    );
    assert!(errors.iter().any(|m| m.contains("Unknown validator 'Nope'")), "{errors:?}");
}

#[test]
fn validators_above_a_struct_are_misplaced() {
    let errors = messages(&format!("{BOUNDS}@validators = [Bounds()]\nstruct S {{\n    x: string\n}}\n"));
    assert_eq!(errors, ["`@validators` goes on a field or a `type` alias"]);
}

#[test]
fn a_deprecated_alias_warns_where_it_is_used() {
    let units = IncrementalInterpreter::from_source(
        "@deprecated = \"use Handle\"\ntype Nick = string\nstruct User {\n    nick: Nick\n}\n",
    );
    let warnings = validate(&units).expect("deprecation is a warning, not an error");
    let messages: Vec<&str> = warnings.iter().map(|w| w.message.as_str()).collect();
    assert_eq!(messages, ["'Nick' is deprecated: use Handle"]);
}
