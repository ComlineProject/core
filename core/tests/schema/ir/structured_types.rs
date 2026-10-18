// Types freeze as structured `TypeRef`s and values as `Literal`s (#95):
// nothing about a type is lost to text, and what the text used to hide -
// floats, fixed array lengths - is now checked and diffed.

use comline_core::schema::ir::compiler::interpreter::incremental::IncrementalInterpreter;
use comline_core::schema::ir::compiler::Compile;
use comline_core::schema::ir::diff::{analyze_schema_changes, BreakingChange};
use comline_core::schema::ir::frozen::types::{
    CompareOp, Comparison, Condition, Literal, LogicalOp, Operand, PrimitiveType as P, TypeRef,
};
use comline_core::schema::ir::frozen::unit::FrozenUnit;
use comline_core::schema::ir::validation::validate;

fn first_field_type(units: &[FrozenUnit]) -> &TypeRef {
    units
        .iter()
        .find_map(|u| match u {
            FrozenUnit::Struct { fields, .. } => match &fields[0] {
                FrozenUnit::Field { ty, .. } => Some(ty),
                _ => None,
            },
            _ => None,
        })
        .expect("a struct with a field")
}

fn messages(code: &str) -> Vec<String> {
    validate(&IncrementalInterpreter::from_source(code))
        .err()
        .unwrap_or_default()
        .into_iter()
        .map(|e| e.message)
        .collect()
}

#[test]
fn floats_keep_their_width_and_validate() {
    let units = IncrementalInterpreter::from_source("struct S {\n    x: f32\n    y: f64\n}\n");
    let FrozenUnit::Struct { fields, .. } = &units[0] else { panic!() };
    let types: Vec<&TypeRef> = fields
        .iter()
        .map(|f| match f {
            FrozenUnit::Field { ty, .. } => ty,
            _ => panic!(),
        })
        .collect();
    assert_eq!(types, [&TypeRef::Primitive(P::F32), &TypeRef::Primitive(P::F64)]);
    assert!(validate(&units).is_ok());
}

#[test]
fn a_fixed_array_keeps_its_length() {
    let units = IncrementalInterpreter::from_source("struct S {\n    digest: u8[32]\n    tags: string[]\n}\n");
    assert_eq!(
        *first_field_type(&units),
        TypeRef::Array { element: Box::new(TypeRef::Primitive(P::U8)), length: Some(32) }
    );
    assert_eq!(first_field_type(&units).to_string(), "u8[32]");
}

#[test]
fn a_fixed_array_of_itself_is_a_cycle() {
    // A dynamic array can be empty, so it ends the recursion; four of a
    // struct inside itself never ends.
    assert!(messages("struct Node {\n    next: Node[]\n}\n").is_empty());
    let errors = messages("struct Node {\n    next: Node[4]\n}\n");
    assert!(errors.iter().any(|m| m.contains("Cycle detected")), "{errors:?}");
}

#[test]
fn a_literal_of_the_wrong_kind_is_rejected() {
    let errors = messages("struct S {\n    n: u8 = \"seven\"\n}\n");
    assert_eq!(errors, ["expected a `u8` value, found a string `\"seven\"`"]);
    let errors = messages("const ON: bool = 1\n");
    assert_eq!(errors, ["expected a `bool` value, found an integer `1`"]);
    // An integer is a fine float.
    assert!(messages("const RATIO: f32 = 2\n").is_empty());
}

#[test]
fn a_reference_default_is_kept_not_dropped() {
    let units = IncrementalInterpreter::from_source(
        "const MAX: u32 = 9\nstruct S {\n    n: u32 = MAX\n}\n",
    );
    let default = units.iter().find_map(|u| match u {
        FrozenUnit::Struct { fields, .. } => match &fields[0] {
            FrozenUnit::Field { default, .. } => Some(default.clone()),
            _ => None,
        },
        _ => None,
    });
    assert_eq!(default, Some(Some(Literal::Reference("MAX".to_string()))));
    assert!(validate(&units).is_ok());
}

#[test]
fn a_constant_needs_a_literal_value() {
    let errors = messages("const LIMIT: u32 = OTHER\n");
    assert_eq!(errors, ["Constant 'LIMIT' must have a literal value"]);
}

#[test]
fn width_and_length_changes_are_breaking() {
    let old = IncrementalInterpreter::from_source("struct S {\n    x: f32\n    d: u8[4]\n}\n");
    let new = IncrementalInterpreter::from_source("struct S {\n    x: f64\n    d: u8[8]\n}\n");
    let changes = analyze_schema_changes(&old, &new);
    let changed: Vec<(String, String, String)> = changes
        .breaking_changes
        .iter()
        .filter_map(|c| match c {
            BreakingChange::ChangedFieldType { field_name, old_type, new_type, .. } => {
                Some((field_name.clone(), old_type.clone(), new_type.clone()))
            }
            _ => None,
        })
        .collect();
    assert!(changed.contains(&("x".into(), "f32".into(), "f64".into())), "{changed:?}");
    assert!(changed.contains(&("d".into(), "u8[4]".into(), "u8[8]".into())), "{changed:?}");
}

#[test]
fn an_assert_condition_freezes_as_a_tree() {
    let units = IncrementalInterpreter::from_source(
        "validator Range for u32 {\n    min: u32 = 0\n    validate {\n        \
         assert(value >= params.min and value < 100, f\"{value.name} is out of range\")\n    }\n}\n",
    );
    let FrozenUnit::Validator { expression_block, .. } = &units[0] else { panic!() };
    let FrozenUnit::ExpressionBlock { asserts } = &**expression_block else { panic!() };
    let FrozenUnit::Assert { expr, condition, .. } = &asserts[0] else { panic!() };
    let path = |p: &str| Operand::Path(p.split('.').map(str::to_string).collect());
    assert_eq!(
        *expr,
        Condition {
            first: Comparison { left: path("value"), op: CompareOp::Ge, right: path("params.min") },
            rest: vec![(
                LogicalOp::And,
                Comparison { left: path("value"), op: CompareOp::Lt, right: Operand::Integer(100) },
            )],
        }
    );
    // The text stays alongside, for messages.
    assert_eq!(condition, "value >= params.min and value < 100");
}

#[test]
fn a_property_without_a_default_is_still_type_checked() {
    let errors = messages(
        "validator V for string {\n    n: u32\n}\n\
         struct S {\n    @validators = [V(n = \"x\")]\n    f: string\n}\n",
    );
    assert!(
        errors.iter().any(|m| m.contains("argument 'n' expects")),
        "{errors:?}"
    );
}

#[test]
fn removed_str_gets_its_hint_without_a_did_you_mean() {
    // `str` is one edit from `s8`; the suggestion contradicted the hint.
    let errors = messages("struct S {\n    name: str\n}\n");
    assert_eq!(errors, ["Unknown type 'str'"]);
}
