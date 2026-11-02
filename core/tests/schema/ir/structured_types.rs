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
        TypeRef::Array { element: Box::new(TypeRef::Primitive(P::U8)), length: Some(32), max: None }
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

// A reference is judged by the value it stands for (core#168): a constant,
// or an integer type's bound.

#[test]
fn a_constant_can_name_another_constant_or_a_bound() {
    assert!(messages("const A: u32 = 5\nconst B: u32 = A\n").is_empty());
    assert!(messages("const A: u32 = 5\nconst B: u32 = A\nconst C: u8 = B\n").is_empty());
    assert!(messages("const M: u32 = u32::MAX\nconst N: s8 = s8::MIN\n").is_empty());
    assert!(messages("const M: u64 = u64::MAX\n").is_empty());
    // A narrower constant is fine while the value fits.
    assert!(messages("const A: u32 = 200\nconst B: u8 = A\n").is_empty());
}

#[test]
fn a_constant_naming_nothing_is_an_error() {
    let errors = messages("const LIMIT: u32 = OTHER\n");
    assert_eq!(errors, ["no constant named `OTHER`"]);
    let errors = messages("const LIMIT: u32 = 1\nconst B: u32 = LIMT\n");
    assert_eq!(errors, ["no constant named `LIMT` - did you mean `LIMIT`?"]);
}

#[test]
fn a_constant_value_has_to_fit_where_it_is_named() {
    let errors = messages("const A: u32 = 500\nconst B: u8 = A\n");
    assert_eq!(errors, ["`A` is 500, which does not fit in `u8`"]);
    let errors = messages("const B: u8 = u32::MAX\n");
    assert_eq!(errors, ["`u32::MAX` is 4294967295, which does not fit in `u8`"]);
    let errors = messages("const B: u8 = s8::MIN\n");
    assert_eq!(errors, ["`s8::MIN` is -128, which does not fit in `u8`"]);
    let errors = messages("const NAME: string = \"x\"\nconst B: u32 = NAME\n");
    assert_eq!(errors, ["expected a `u32` value, found `NAME`, which is a `string`"]);
    // An integer is a fine float, a float isn't an integer.
    assert!(messages("const A: u32 = 5\nconst F: f64 = A\n").is_empty());
    let errors = messages("const F: f32 = 5\nconst A: u32 = F\n");
    assert_eq!(errors, ["expected a `u32` value, found `F`, which is a `f32`"]);
}

#[test]
fn only_an_integer_type_has_bounds() {
    let errors = messages("const F: f32 = f32::MAX\n");
    assert_eq!(errors, ["`f32` has no `MAX`"]);
    let errors = messages("const A: u32 = u32::LIMIT\n");
    assert_eq!(errors, ["`u32` has no `LIMIT`"]);
}

#[test]
fn a_constant_naming_something_else_is_an_error() {
    let errors = messages("struct T {\n    a: u8\n}\nconst A: u32 = T\n");
    assert_eq!(errors, ["`T` is a struct, not a constant"]);
}

#[test]
fn constants_naming_each_other_in_a_loop_are_an_error() {
    let errors = messages("const A: u32 = B\nconst B: u32 = A\nstruct S {\n    n: u32 = A\n}\n");
    // Reported at each constant on the loop; the field naming one adds
    // nothing.
    assert_eq!(
        errors,
        [
            "constant 'A' refers back to itself: A -> B -> A",
            "constant 'B' refers back to itself: B -> A -> B",
        ]
    );
    let errors = messages("const A: u32 = A\n");
    assert_eq!(errors, ["constant 'A' refers back to itself: A -> A"]);
    // A constant that leads into a loop isn't on it.
    let errors = messages("const A: u32 = B\nconst B: u32 = B\n");
    assert_eq!(errors, ["constant 'B' refers back to itself: B -> B"]);
}

#[test]
fn a_constant_whose_own_value_is_wrong_is_reported_once() {
    // Where it's declared, not again where it's named.
    let errors = messages("const A: u8 = 500\nconst B: u32 = A\nstruct S {\n    n: u32 = A\n}\n");
    assert_eq!(errors, ["the value 500 does not fit in `u8`"]);
}

#[test]
fn a_default_naming_a_constant_is_checked() {
    let errors = messages("const A: u32 = 500\nstruct S {\n    a: u8 = A\n    b: u8 = NOPE\n    c: u32 = A\n}\n");
    assert_eq!(
        errors,
        ["`A` is 500, which does not fit in `u8`", "no constant named `NOPE`"]
    );
    let errors = messages("struct S {\n    a: u8 = u16::MAX\n    b: u16 = u16::MAX\n}\n");
    assert_eq!(errors, ["`u16::MAX` is 65535, which does not fit in `u8`"]);
}

#[test]
fn a_default_through_an_alias_is_checked() {
    let errors = messages("type Port = u16\nstruct S {\n    p: Port = 70000\n    q: Port = 80\n}\n");
    assert_eq!(errors, ["the value 70000 does not fit in `u16`"]);
}

#[test]
fn an_error_field_default_naming_a_constant_is_checked() {
    let errors = messages(
        "const A: u32 = 500\nerror E {\n    message = \"e\"\n    a: u8 = A\n}\n",
    );
    assert_eq!(errors, ["`A` is 500, which does not fit in `u8`"]);
}

#[test]
fn validator_values_naming_a_constant_are_checked() {
    let validator = "validator Short for string {\n    max: u32 = DEFAULT\n    validate {\n        \
                     assert(value.length <= params.max, f\"{value.name} is too long\")\n    }\n}\n";
    // A property default.
    let errors = messages(&format!("const DEFAULT: u32 = 10\n{validator}"));
    assert!(errors.is_empty(), "{errors:?}");
    let errors = messages(&format!("const DEFAULT: string = \"ten\"\n{validator}"));
    assert_eq!(errors, ["expected a `u32` value, found `DEFAULT`, which is a `string`"]);
    // A call's argument, by name or as an integer.
    let field = |argument: &str| {
        format!(
            "const DEFAULT: u32 = 10\nconst NAME: string = \"x\"\n{validator}\
             struct S {{\n    @validators = [Short(max = {argument})]\n    a: string\n}}\n"
        )
    };
    assert!(messages(&field("DEFAULT")).is_empty());
    assert!(messages(&field("u32::MAX")).is_empty());
    assert_eq!(messages(&field("NAME")), ["expected a `u32` value, found `NAME`, which is a `string`"]);
    assert_eq!(messages(&field("MISSING")), ["no constant named `MISSING`"]);
    assert_eq!(messages(&field("-1")), ["the value -1 does not fit in `u32`"]);
}

#[test]
fn a_default_can_be_the_types_own_default() {
    let code = "struct S {\n    n: u8 = default\n    s: string = default\n}\n";
    assert!(messages(code).is_empty(), "{:?}", messages(code));
    // A constant states its value.
    assert_eq!(messages("const A: u8 = default\n"), ["no constant named `default`"]);
}

#[test]
fn a_default_for_an_enum_is_not_read_as_a_constant() {
    let code = "enum Color {\n    Red\n}\nstruct S {\n    c: Color = Red\n}\n";
    assert!(messages(code).iter().all(|m| !m.contains("constant")), "{:?}", messages(code));
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
