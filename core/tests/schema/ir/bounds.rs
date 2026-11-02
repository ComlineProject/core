// Bounds on types (core#98): `T[max N]`, `char`, sizes from constants, and
// the checks on what's written.

use comline_core::schema::idl::grammar;
use comline_core::schema::ir::compiler::interpreter::incremental::IncrementalInterpreter;
use comline_core::schema::ir::frozen::types::{PrimitiveType as P, TypeRef};
use comline_core::schema::ir::frozen::unit::FrozenUnit;

fn compile(code: &str) -> Result<Vec<FrozenUnit>, Vec<String>> {
    let declarations = grammar::parse(code).expect("should parse").0;
    IncrementalInterpreter::check_and_from_declarations(declarations, None)
        .map_err(|errors| errors.into_iter().map(|e| e.message).collect())
}

/// Each field's type, in order, of the struct `S`.
fn field_types(code: &str) -> Vec<TypeRef> {
    compile(code)
        .unwrap_or_else(|e| panic!("should build: {e:?}"))
        .into_iter()
        .find_map(|u| match u {
            FrozenUnit::Struct { name, fields, .. } if name == "S" => Some(
                fields
                    .into_iter()
                    .filter_map(|f| match f {
                        FrozenUnit::Field { ty, .. } => Some(ty),
                        _ => None,
                    })
                    .collect(),
            ),
            _ => None,
        })
        .expect("a struct S")
}

fn array(element: TypeRef, length: Option<u64>, max: Option<u64>) -> TypeRef {
    TypeRef::Array { element: Box::new(element), length, max }
}

fn errors(code: &str) -> Vec<String> {
    compile(code).err().unwrap_or_default()
}

#[test]
fn a_string_is_an_array_of_char() {
    let types = field_types(
        "struct S {\n    a: char[max 32]\n    b: char[2]\n    c: char[]\n    d: string\n    e: char\n}\n",
    );
    let char = || TypeRef::Primitive(P::Char);
    assert_eq!(
        types,
        [
            array(char(), None, Some(32)),
            array(char(), Some(2), None),
            // One stored form for a string with no bound.
            TypeRef::Primitive(P::String),
            TypeRef::Primitive(P::String),
            char(),
        ]
    );
}

#[test]
fn any_dynamic_array_takes_a_maximum() {
    let types = field_types("struct S {\n    a: u8[max 10]\n    b: string[max 3]\n    c: char[max 8][max 4]\n}\n");
    assert_eq!(
        types,
        [
            array(TypeRef::Primitive(P::U8), None, Some(10)),
            array(TypeRef::Primitive(P::String), None, Some(3)),
            array(array(TypeRef::Primitive(P::Char), None, Some(8)), None, Some(4)),
        ]
    );
}

#[test]
fn a_size_can_be_a_constant_and_is_stored_resolved() {
    let types = field_types(
        "const NAME: u32 = 16\nconst TWICE: u32 = NAME\nstruct S {\n    a: char[max NAME]\n    b: u8[TWICE]\n    \
         c: u8[max u16::MAX]\n}\n",
    );
    assert_eq!(
        types,
        [
            array(TypeRef::Primitive(P::Char), None, Some(16)),
            array(TypeRef::Primitive(P::U8), Some(16), None),
            array(TypeRef::Primitive(P::U8), None, Some(65535)),
        ]
    );
}

#[test]
fn max_is_a_keyword_only_inside_brackets() {
    let code = "const max: u32 = 3\nstruct S {\n    max: u32\n    a: char[max max]\n}\n\
                validator Short for string {\n    max: u32 = 10\n    validate {\n        \
                assert(value.length <= params.max, f\"{value.name} is too long\")\n    }\n}\n";
    let types = field_types(code);
    assert_eq!(types, [TypeRef::Primitive(P::U32), array(TypeRef::Primitive(P::Char), None, Some(3))]);
}

#[test]
fn a_type_without_a_bound_freezes_to_the_same_bytes() {
    // `max` is left out when absent, so an existing schema's stored form,
    // and its hash, don't change.
    let unbounded = serde_json::to_string(&array(TypeRef::Primitive(P::U8), Some(4), None)).unwrap();
    assert!(!unbounded.contains("max"), "{unbounded}");
    let bounded = serde_json::to_string(&array(TypeRef::Primitive(P::U8), None, Some(4))).unwrap();
    assert!(bounded.contains("\"max\":4"), "{bounded}");
}

#[test]
fn a_size_is_at_least_1_and_at_most_u32_max() {
    assert_eq!(errors("struct S {\n    a: char[max 0]\n}\n"), ["a maximum of 0: a size is at least 1"]);
    assert_eq!(errors("struct S {\n    a: u8[0]\n}\n"), ["a length of 0: a size is at least 1"]);
    assert_eq!(errors("struct S {\n    a: u8[max -3]\n}\n"), ["a maximum of -3: a size is at least 1"]);
    assert_eq!(
        errors("struct S {\n    a: u8[max 4294967296]\n}\n"),
        ["a maximum of 4294967296 is over 4294967295"]
    );
    assert_eq!(
        errors("const NONE: u32 = 0\nstruct S {\n    a: u8[max NONE]\n}\n"),
        ["a maximum of 0 (`NONE`): a size is at least 1"]
    );
    assert!(errors("struct S {\n    a: u8[max 4294967295]\n}\n").is_empty());
}

#[test]
fn a_size_names_an_unsigned_integer_constant() {
    assert_eq!(
        errors("struct S {\n    a: u8[max NOPE]\n}\n"),
        ["the maximum `NOPE` isn't an unsigned integer constant"]
    );
    assert_eq!(
        errors("const N: s32 = 4\nstruct S {\n    a: u8[max N]\n}\n"),
        ["the maximum `N` isn't an unsigned integer constant"]
    );
    assert_eq!(
        errors("const N: string = \"4\"\nstruct S {\n    a: u8[N]\n}\n"),
        ["the length `N` isn't an unsigned integer constant"]
    );
    assert_eq!(
        errors("struct S {\n    a: u8[max s8::MAX]\n}\n"),
        ["the maximum `s8::MAX` isn't an unsigned integer constant"]
    );
}

#[test]
fn an_alias_of_a_string_or_list_takes_no_suffix_where_used() {
    let aliases = "type Name = char[max 32]\ntype Tags = u8[]\ntype Code = u8[4]\ntype Id = u32\nstruct T {\n    a: u8\n}\n";
    assert_eq!(
        errors(&format!("{aliases}struct S {{\n    a: Name[max 3]\n}}\n")),
        ["`Name` is an alias of a string, which takes no `[…]` where it's used"]
    );
    assert_eq!(
        errors(&format!("{aliases}protocol P {{\n    function f(t: Tags[2]) -> union(u8 Tags[]);\n}}\n")),
        [
            "`Tags` is an alias of a list, which takes no `[…]` where it's used",
            "`Tags` is an alias of a list, which takes no `[…]` where it's used",
        ]
    );
    // A list of it is an alias of its own; numbers, structs, fixed arrays
    // and the built-in `string` keep the suffix.
    let fine = format!(
        "{aliases}type Names = Name[max 3]\nstruct S {{\n    a: Names\n    b: Id[max 4]\n    c: T[2]\n    \
         d: Code[]\n    e: string[max 10]\n}}\n"
    );
    assert!(errors(&fine).is_empty(), "{:?}", errors(&fine));
}

#[test]
fn a_validator_matches_whatever_the_bound() {
    let code = "validator Short for string {\n    max: u32 = 10\n    validate {\n        \
                assert(value.length <= params.max, f\"{value.name} is too long\")\n    }\n}\n\
                validator Few for u32[] {\n    validate {\n        \
                assert(value.length <= 3, f\"{value.name} has too many\")\n    }\n}\n\
                struct S {\n    @validators = [Short()]\n    a: char[max 32]\n    \
                @validators = [Short()]\n    b: char[4]\n    @validators = [Few()]\n    c: u32[max 8]\n}\n";
    let units = compile(code).unwrap_or_else(|e| panic!("should build: {e:?}"));
    let result = comline_core::schema::ir::validation::validate(&units);
    assert!(result.is_ok(), "{:?}", result.err());
}

#[test]
fn a_default_has_to_fit_its_bound() {
    let messages = |code: &str| -> Vec<String> {
        let units = compile(code).unwrap_or_else(|e| panic!("should build: {e:?}"));
        comline_core::schema::ir::validation::validate(&units)
            .err()
            .unwrap_or_default()
            .into_iter()
            .map(|e| e.message)
            .collect()
    };
    assert!(messages("struct S {\n    a: char[max 3] = \"abc\"\n    b: char[2] = \"é😀\"\n    c: char = \"x\"\n}\n").is_empty());
    assert_eq!(
        messages("struct S {\n    a: char[max 3] = \"abcd\"\n}\n"),
        ["the default \"abcd\" is 4 characters; the type holds at most 3"]
    );
    assert_eq!(
        messages("const NAME: string = \"x\"\nstruct S {\n    a: char[2] = NAME\n}\n"),
        ["the default \"x\" is 1 character; the type holds exactly 2"]
    );
    assert_eq!(
        messages("struct S {\n    c: char = \"xy\"\n}\n"),
        ["a `char` is one character, found 2 in \"xy\""]
    );
    assert_eq!(
        messages("struct S {\n    a: char[max 3] = 7\n}\n"),
        ["expected a `string` value, found an integer `7`"]
    );
}

#[test]
fn any_change_to_a_bound_is_breaking() {
    use comline_core::schema::ir::diff::analyze_schema_changes;
    let units = |ty: &str| compile(&format!("struct S {{\n    a: {ty}\n}}\n")).unwrap();
    for (old, new) in [
        ("char[max 32]", "char[max 64]"),
        ("char[max 64]", "char[max 32]"),
        ("string", "char[max 32]"),
        ("u8[max 4]", "u8[]"),
        ("char[2]", "char[max 2]"),
    ] {
        let changes = analyze_schema_changes(&units(old), &units(new));
        assert!(!changes.breaking_changes.is_empty(), "{old} -> {new}: {changes:?}");
    }
    // The same bound, spelled through a constant, is no change.
    let constant = compile("const N: u32 = 32\nstruct S {\n    a: char[max N]\n}\n").unwrap();
    assert!(analyze_schema_changes(&units("char[max 32]"), &constant).breaking_changes.is_empty());
}

// Default bounds from settings (core#98): stored resolved at each position.

#[test]
fn a_settings_default_fills_every_position_without_a_bound() {
    let types = field_types(
        "settings {\n    bounds.string.max = 64\n    bounds.array.max = 100\n}\n\
         struct S {\n    a: string\n    b: u8[]\n    c: char[max 8]\n    d: u8[4]\n    e: string[]\n    \
         f: u8[max 2]\n}\n",
    );
    let char64 = array(TypeRef::Primitive(P::Char), None, Some(64));
    assert_eq!(
        types,
        [
            char64.clone(),
            array(TypeRef::Primitive(P::U8), None, Some(100)),
            // Written in the type: kept.
            array(TypeRef::Primitive(P::Char), None, Some(8)),
            array(TypeRef::Primitive(P::U8), Some(4), None),
            array(char64, None, Some(100)),
            array(TypeRef::Primitive(P::U8), None, Some(2)),
        ]
    );
}

#[test]
fn a_default_at_a_position_beats_the_schema_wide_one() {
    let types = field_types(
        "settings {\n    bounds.string.max = 64\n    struct.field.bounds.string.max = 16\n}\n\
         settings Long {\n    bounds.string.max = 4096\n}\n\
         struct S {\n    a: string\n    @settings = Long\n    b: string\n}\n",
    );
    let char = |n| array(TypeRef::Primitive(P::Char), None, Some(n));
    // The scoped key beats the global one, in the preset as in the schema.
    assert_eq!(types, [char(16), char(16)]);

    let types = field_types(
        "settings {\n    bounds.string.max = 64\n}\nsettings Long {\n    bounds.string.max = 4096\n}\n\
         struct S {\n    a: string\n    @settings = Long\n    b: string\n}\n",
    );
    assert_eq!(types, [char(64), char(4096)]);
}

#[test]
fn without_a_default_nothing_changes() {
    assert_eq!(field_types("struct S {\n    a: string\n    b: u8[]\n}\n"), [
        TypeRef::Primitive(P::String),
        array(TypeRef::Primitive(P::U8), None, None),
    ]);
}

#[test]
fn a_struct_that_contains_itself_takes_the_default_depth() {
    let units = compile(
        "settings {\n    bounds.depth.max = 32\n}\n\
         struct Node {\n    kids: Node[]\n}\nstruct Leaf {\n    a: u8\n}\n\
         @max_depth = 4\nstruct Tree {\n    optional next: Tree\n}\n",
    )
    .unwrap();
    let depth = |name: &str| {
        units.iter().find_map(|u| match u {
            FrozenUnit::Struct { name: n, parameters, .. } if n == name => Some(
                parameters
                    .iter()
                    .filter_map(|p| match p {
                        FrozenUnit::Property { name, value } if name == "max_depth" => Some(value.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
    };
    use comline_core::schema::ir::frozen::unit::ConfigValue;
    assert_eq!(depth("Node"), Some(vec![ConfigValue::Integer(32)]));
    assert_eq!(depth("Leaf"), Some(vec![]));
    // Written: kept, and not added to.
    assert_eq!(depth("Tree"), Some(vec![ConfigValue::Integer(4)]));
}

#[test]
fn a_default_bound_is_a_known_kind_and_a_size() {
    let messages = errors("settings {\n    bounds.bytes.max = 3\n}\n");
    assert_eq!(messages, ["unrecognised settings key 'bounds.bytes.max'"]);
    let messages = errors("settings {\n    bounds.string.max = 0\n}\n");
    assert_eq!(messages, ["'bounds.string.max' is 0: a maximum is from 1 to 4294967295"]);
    let messages = errors("settings {\n    bounds.string.max = \"64\"\n}\n");
    assert_eq!(messages.len(), 1, "{messages:?}");
}

#[test]
fn union_members_differing_only_by_bound_are_an_error() {
    let messages = |code: &str| -> Vec<String> {
        let units = compile(code).unwrap_or_else(|e| panic!("should build: {e:?}"));
        comline_core::schema::ir::validation::validate(&units)
            .err()
            .unwrap_or_default()
            .into_iter()
            .map(|e| e.message)
            .collect()
    };
    assert_eq!(
        messages("struct S {\n    a: union(char[max 3] char[max 5])\n}\n"),
        ["union members `char[max 3]` and `char[max 5]` differ only by bound"]
    );
    assert_eq!(
        messages("struct S {\n    a: union(u8[max 3] u8[])\n}\n"),
        ["union members `u8[max 3]` and `u8[]` differ only by bound"]
    );
    assert!(messages("struct S {\n    a: union(char[max 3] u8[max 3] u32)\n}\n").is_empty());
}
