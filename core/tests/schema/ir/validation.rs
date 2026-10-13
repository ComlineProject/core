// IR validation tests - verify actual FrozenUnit content

use comline_core::schema::idl::grammar;
use comline_core::schema::ir::compiler::interpreter::incremental::IncrementalInterpreter;
use comline_core::schema::ir::compiler::Compile;

#[cfg(test)]
mod ir_validation_tests {
    use super::*;
    use comline_core::schema::ir::compiler::interpreter::incremental::IncrementalInterpreter;
    use comline_core::schema::ir::compiler::Compile;
    use comline_core::schema::ir::validation::validate;

    // These tests would ideally validate the actual IR content,
    // but since from_declarations -> (), we verify no panics occur

    #[test]
    fn test_struct_field_types() {
        let code = r#"
struct TestStruct {
    id: u64
    name: str
    count: u32
    active: bool
}
"#;
        let parsed = grammar::parse(code);
        assert!(parsed.is_ok());

        // Verify IR generation content
        let ir_units = IncrementalInterpreter::from_source(code);
        assert_eq!(ir_units.len(), 1);
        match &ir_units[0] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Struct { name, fields, .. } => {
                assert_eq!(name, "TestStruct");
                assert_eq!(fields.len(), 4);
                // We could check individual field types here
            }
            _ => panic!("Expected Struct"),
        }
    }

    #[test]
    fn test_struct_and_field_docstrings_populate_ir() {
        let code = r#"
/// A user account.
struct User {
    /// The user's unique id.
    id: u64
    name: str
}
"#;
        let ir_units = IncrementalInterpreter::from_source(code);
        match &ir_units[0] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Struct {
                docstring,
                fields,
                ..
            } => {
                assert_eq!(docstring.as_deref(), Some("A user account."));

                match &fields[0] {
                    comline_core::schema::ir::frozen::unit::FrozenUnit::Field {
                        docstring, ..
                    } => {
                        assert_eq!(docstring.as_deref(), Some("The user's unique id."));
                    }
                    _ => panic!("Expected Field"),
                }
                // A field with no docstring keeps behaving exactly as
                // before the shared build_kind_value/docstring wiring.
                match &fields[1] {
                    comline_core::schema::ir::frozen::unit::FrozenUnit::Field {
                        docstring, ..
                    } => {
                        assert_eq!(*docstring, None);
                    }
                    _ => panic!("Expected Field"),
                }
            }
            _ => panic!("Expected Struct"),
        }
    }

    #[test]
    fn test_multi_line_docstring_is_newline_joined() {
        let code = r#"
/// Checks if a string length is within bounds.
/// @min_chars: Minimum length of the string.
struct StringBounds {
    min_chars: u32
}
"#;
        let ir_units = IncrementalInterpreter::from_source(code);
        match &ir_units[0] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Struct { docstring, .. } => {
                assert_eq!(
                    docstring.as_deref(),
                    Some(
                        "Checks if a string length is within bounds.\n@min_chars: Minimum length of the string."
                    )
                );
            }
            _ => panic!("Expected Struct"),
        }
    }

    #[test]
    fn test_protocol_and_function_docstrings_populate_ir() {
        let code = r#"
/// A user service.
protocol UserService {
    /// Get a user by id.
    function get(u64) -> str;
}
"#;
        let ir_units = IncrementalInterpreter::from_source(code);
        match &ir_units[0] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Protocol {
                docstring,
                functions,
                ..
            } => {
                assert_eq!(docstring, "A user service.");

                match &functions[0] {
                    comline_core::schema::ir::frozen::unit::FrozenUnit::Function {
                        docstring, ..
                    } => {
                        assert_eq!(docstring, "Get a user by id.");
                    }
                    _ => panic!("Expected Function"),
                }
            }
            _ => panic!("Expected Protocol"),
        }
    }

    #[test]
    fn test_enum_and_const_docstrings_populate_ir() {
        let code = r#"
/// Status values.
enum Status {
    Active
}

/// The max allowed.
const MAX: u32 = 10
"#;
        let ir_units = IncrementalInterpreter::from_source(code);
        match &ir_units[0] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Enum { docstring, .. } => {
                assert_eq!(docstring.as_deref(), Some("Status values."));
            }
            _ => panic!("Expected Enum"),
        }
        match &ir_units[1] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Constant { docstring, .. } => {
                assert_eq!(docstring.as_deref(), Some("The max allowed."));
            }
            _ => panic!("Expected Constant"),
        }
    }

    #[test]
    fn test_error_declaration_populates_ir() {
        let code = r#"
/// Thrown when a user can't be found.
error NotFoundError {
    message = "{self.id} was not found"
    id: u64
}
"#;
        let ir_units = IncrementalInterpreter::from_source(code);
        match &ir_units[0] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Error {
                docstring,
                name,
                message,
                fields,
                ..
            } => {
                assert_eq!(
                    docstring.as_deref(),
                    Some("Thrown when a user can't be found.")
                );
                assert_eq!(name, "NotFoundError");
                // The placeholder round-trips back to its original,
                // un-escaped single-brace form.
                assert_eq!(message, "{self.id} was not found");
                assert_eq!(fields.len(), 1);
            }
            _ => panic!("Expected Error"),
        }
    }

    #[test]
    fn test_error_message_escaped_braces_round_trip() {
        let code = "error E {\n    message = \"code: {{404}}\"\n}";
        let ir_units = IncrementalInterpreter::from_source(code);
        match &ir_units[0] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Error { message, .. } => {
                assert_eq!(message, "code: {404}");
            }
            _ => panic!("Expected Error"),
        }
    }

    #[test]
    fn test_function_throws_populates_ir() {
        use comline_core::schema::ir::frozen::unit::FrozenUnit;

        // `NotFoundError` is neither declared locally nor imported: it still
        // gets a stable ordinal (0), and an `<unresolved: ...>` re-export slot
        // is appended so the ordinal has a home in the IR.
        let code = "protocol P {\n    function get(u64) -> str ! NotFoundError;\n}";
        let ir_units = IncrementalInterpreter::from_source(code);

        let FrozenUnit::Protocol { functions, .. } = &ir_units[0] else {
            panic!("Expected Protocol");
        };
        let FrozenUnit::Function { throws, .. } = &functions[0] else {
            panic!("Expected Function");
        };
        assert_eq!(throws, &vec![0u16]);

        let reexport = ir_units
            .iter()
            .find_map(|u| match u {
                FrozenUnit::Error { name, ordinal, imported_from, .. } if name == "NotFoundError" => {
                    Some((*ordinal, imported_from.clone()))
                }
                _ => None,
            })
            .expect("an Error unit for the thrown name");
        assert_eq!(reexport.0, 0);
        assert_eq!(reexport.1.as_deref(), Some("<unresolved: NotFoundError>"));
    }

    #[test]
    fn test_function_without_throws_still_empty_vec() {
        // A function with no throws clause still gets an empty Vec (now
        // Vec<u16>, resolved from the `! Name` references).
        let code = "protocol P {\n    function get(u64) -> str;\n}";
        let ir_units = IncrementalInterpreter::from_source(code);
        match &ir_units[0] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Protocol { functions, .. } => {
                match &functions[0] {
                    comline_core::schema::ir::frozen::unit::FrozenUnit::Function {
                        throws,
                        ..
                    } => {
                        assert!(throws.is_empty());
                    }
                    _ => panic!("Expected Function"),
                }
            }
            _ => panic!("Expected Protocol"),
        }
    }

    #[test]
    fn test_local_error_gets_ordinal_and_throws_resolves_to_it() {
        use comline_core::schema::ir::frozen::unit::FrozenUnit;

        let code = "\
error Missing {\n    message = \"gone\"\n}\n\
error Denied {\n    message = \"no\"\n}\n\
protocol P {\n    function get(u64) -> str ! Denied;\n}";
        let ir_units = IncrementalInterpreter::from_source(code);

        // Declaration order: Missing = 0, Denied = 1.
        let ordinal_of = |want: &str| {
            ir_units.iter().find_map(|u| match u {
                FrozenUnit::Error { name, ordinal, imported_from: None, .. } if name == want => {
                    Some(*ordinal)
                }
                _ => None,
            })
        };
        assert_eq!(ordinal_of("Missing"), Some(0));
        assert_eq!(ordinal_of("Denied"), Some(1));

        let FrozenUnit::Protocol { functions, .. } =
            ir_units.iter().find(|u| matches!(u, FrozenUnit::Protocol { .. })).unwrap()
        else {
            unreachable!()
        };
        let FrozenUnit::Function { throws, .. } = &functions[0] else {
            panic!("Expected Function");
        };
        assert_eq!(throws, &vec![1u16]);
    }

    /// Every unsigned and signed width keeps its own primitive, rather than
    /// collapsing into `U64`/`S64`. The collapse was invisible in core and
    /// loud downstream: a generator renders the primitive's own name, so
    /// `retries: u8 = 5` became `pub retries: u64`.
    #[test]
    fn a_default_keeps_the_width_its_field_declared() {
        use comline_core::schema::ir::compiler::interpreted::kind_search::{KindValue, Primitive};
        use comline_core::schema::ir::frozen::unit::FrozenUnit;

        let code = r#"
struct Widths {
    a: u8 = 1
    b: u16 = 2
    c: u32 = 3
    d: u64 = 4
    e: s8 = -1
    f: s16 = -2
    g: s32 = -3
    h: s64 = -4
}
"#;
        let ir_units = IncrementalInterpreter::from_source(code);
        let FrozenUnit::Struct { fields, .. } = &ir_units[0] else {
            panic!("expected a struct");
        };
        let kinds: Vec<&KindValue> = fields
            .iter()
            .map(|f| match f {
                FrozenUnit::Field { kind_value, .. } => kind_value,
                _ => panic!("expected a field"),
            })
            .collect();
        assert_eq!(
            kinds,
            vec![
                &KindValue::Primitive(Primitive::U8(Some(1))),
                &KindValue::Primitive(Primitive::U16(Some(2))),
                &KindValue::Primitive(Primitive::U32(Some(3))),
                &KindValue::Primitive(Primitive::U64(Some(4))),
                &KindValue::Primitive(Primitive::S8(Some(-1))),
                &KindValue::Primitive(Primitive::S16(Some(-2))),
                &KindValue::Primitive(Primitive::S32(Some(-3))),
                &KindValue::Primitive(Primitive::S64(Some(-4))),
            ]
        );
    }

    /// `= true` froze as `false`: the match arm took `Some(_)` and threw the
    /// literal away. Silent, and it travelled into the IR hash.
    #[test]
    fn a_boolean_default_keeps_the_value_that_was_written() {
        use comline_core::schema::ir::compiler::interpreted::kind_search::{KindValue, Primitive};
        use comline_core::schema::ir::frozen::unit::FrozenUnit;

        let code = r#"
struct Flags {
    on: bool = true
    off: bool = false
}
"#;
        let ir_units = IncrementalInterpreter::from_source(code);
        let FrozenUnit::Struct { fields, .. } = &ir_units[0] else {
            panic!("expected a struct");
        };
        let kinds: Vec<&KindValue> = fields
            .iter()
            .map(|f| match f {
                FrozenUnit::Field { kind_value, .. } => kind_value,
                _ => panic!("expected a field"),
            })
            .collect();
        assert_eq!(
            kinds,
            vec![
                &KindValue::Primitive(Primitive::Boolean(Some(true))),
                &KindValue::Primitive(Primitive::Boolean(Some(false))),
            ]
        );
    }

    /// `u128`/`s128` are primitives the grammar accepts, but no arm mapped
    /// them, so they fell through to `Namespaced` and validation rejected a
    /// legitimate declaration with "cannot be a named type 'u128'".
    #[test]
    fn a_128_bit_constant_is_a_primitive_not_a_named_type() {
        use comline_core::schema::ir::compiler::interpreted::kind_search::{KindValue, Primitive};
        use comline_core::schema::ir::frozen::unit::FrozenUnit;

        let ir_units = IncrementalInterpreter::from_source("const BIG: u128 = 7
");
        let FrozenUnit::Constant { kind_value, .. } = &ir_units[0] else {
            panic!("expected a constant");
        };
        assert_eq!(*kind_value, KindValue::Primitive(Primitive::U128(Some(7))));
        assert!(validate(&IncrementalInterpreter::from_source("const BIG: u128 = 7\n")).is_ok());
    }

    /// A value too large for the width it declares is refused, rather than
    /// truncated into something the author never wrote. Narrowing the
    /// variant to the declared type is what made this reachable: the old
    /// `U64`-for-everything behaviour could hold any of them.
    #[test]
    fn a_value_too_large_for_its_declared_width_is_refused() {
        let errors = validate(&IncrementalInterpreter::from_source("const SMALL: u8 = 300
"))
            .expect_err("300 does not fit in u8");
        assert!(
            errors[0].message.contains("does not fit in `u8`"),
            "got: {}",
            errors[0].message
        );
        assert!(
            !errors[0].message.contains("named type"),
            "an out-of-range value is not a named type: {}",
            errors[0].message
        );
    }

    /// The same check reaches a field default, not just a constant.
    #[test]
    fn an_out_of_range_field_default_is_refused() {
        let errors = validate(&IncrementalInterpreter::from_source(
            "struct S {
    small: u8 = 300
}
",
        ))
        .expect_err("300 does not fit in u8");
        assert!(
            errors.iter().any(|e| e.message.contains("does not fit in `u8`")),
            "got: {:?}",
            errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );
    }

    /// `Primitive::String` reported its type name as the empty string, so a
    /// generator asking the primitive for its name rendered `name: ;` -
    /// output that does not parse in Rust or TypeScript.
    #[test]
    fn a_string_primitive_knows_its_own_type_name() {
        use comline_core::schema::ir::compiler::interpreted::kind_search::Primitive;

        assert_eq!(Primitive::String(None).name(), "string");
        assert_eq!(Primitive::String(Some("x".into())).name(), "string");
    }

    #[test]
    fn test_struct_field_default_values() {
        use comline_core::schema::ir::compiler::interpreted::kind_search::{KindValue, Primitive};

        let code = r#"
struct Config {
    retries: u32 = 3
    label: str = "default"
    timeout: s64 = -1
    name: str
}
"#;
        let parsed = grammar::parse(code);
        assert!(parsed.is_ok());

        let ir_units = IncrementalInterpreter::from_source(code);
        match &ir_units[0] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Struct { fields, .. } => {
                assert_eq!(fields.len(), 4);

                match &fields[0] {
                    comline_core::schema::ir::frozen::unit::FrozenUnit::Field {
                        kind_value, ..
                    } => {
                        // `u32`, not `u64`: the variant comes from the
                        // declared type. This asserted `U64` until core
                        // stopped inferring the variant from the literal -
                        // which is what made `retries: u8 = 5` reach a
                        // generator as `u64`.
                        assert_eq!(
                            *kind_value,
                            KindValue::Primitive(Primitive::U32(Some(3)))
                        );
                    }
                    _ => panic!("Expected Field"),
                }
                match &fields[1] {
                    comline_core::schema::ir::frozen::unit::FrozenUnit::Field {
                        kind_value, ..
                    } => {
                        assert_eq!(
                            *kind_value,
                            KindValue::Primitive(Primitive::String(Some("default".to_string())))
                        );
                    }
                    _ => panic!("Expected Field"),
                }
                match &fields[2] {
                    comline_core::schema::ir::frozen::unit::FrozenUnit::Field {
                        kind_value, ..
                    } => {
                        assert_eq!(
                            *kind_value,
                            KindValue::Primitive(Primitive::S64(Some(-1)))
                        );
                    }
                    _ => panic!("Expected Field"),
                }
                // A field with no default keeps behaving exactly as before
                // the shared build_kind_value refactor.
                match &fields[3] {
                    comline_core::schema::ir::frozen::unit::FrozenUnit::Field {
                        kind_value, ..
                    } => {
                        assert_eq!(
                            *kind_value,
                            KindValue::Namespaced("str".to_string(), None)
                        );
                    }
                    _ => panic!("Expected Field"),
                }
            }
            _ => panic!("Expected Struct"),
        }
    }

    #[test]
    fn test_enum_variants() {
        let code = r#"
enum Color {
    Red
    Green
    Blue
    Yellow
    Black
    White
}
"#;
        let parsed = grammar::parse(code);
        assert!(parsed.is_ok());

        let ir_units = IncrementalInterpreter::from_source(code);
        assert_eq!(ir_units.len(), 1);
        match &ir_units[0] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Enum { name, .. } => {
                assert_eq!(name, "Color");
                // TODO: Verify variants count when exposed
            }
            _ => panic!("Expected Enum"),
        }
    }

    #[test]
    fn test_function_arguments_mapping() {
        let code = r#"
protocol TestService {
    function noArgs() -> str;
    function oneArg(u64) -> bool;
    function twoArgs(str, u32) -> s64;
    function manyArgs(u8, u16, u32, u64, str, bool) -> str;
}
"#;
        let parsed = grammar::parse(code);
        assert!(parsed.is_ok());

        let ir_units = IncrementalInterpreter::from_source(code);
        assert_eq!(ir_units.len(), 1);
        match &ir_units[0] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Protocol {
                name,
                functions,
                ..
            } => {
                assert_eq!(name, "TestService");
                assert_eq!(functions.len(), 4);
            }
            _ => panic!("Expected Protocol"),
        }
    }

    #[test]
    fn test_function_argument_names() {
        let code = r#"
protocol TestService {
    function bareArgs(u64, str) -> bool;
    function namedArgs(id: u64, name: str) -> bool;
}
"#;
        let parsed = grammar::parse(code);
        assert!(parsed.is_ok());

        let ir_units = IncrementalInterpreter::from_source(code);
        match &ir_units[0] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Protocol { functions, .. } => {
                match &functions[0] {
                    comline_core::schema::ir::frozen::unit::FrozenUnit::Function {
                        arguments, ..
                    } => {
                        // No names given - falls back to synthesized arg0/arg1.
                        assert_eq!(arguments[0].name, "arg0");
                        assert_eq!(arguments[1].name, "arg1");
                    }
                    _ => panic!("Expected Function"),
                }
                match &functions[1] {
                    comline_core::schema::ir::frozen::unit::FrozenUnit::Function {
                        arguments, ..
                    } => {
                        // Real names given - used as-is.
                        assert_eq!(arguments[0].name, "id");
                        assert_eq!(arguments[1].name, "name");
                    }
                    _ => panic!("Expected Function"),
                }
            }
            _ => panic!("Expected Protocol"),
        }
    }

    #[test]
    fn test_function_return_types() {
        let code = r#"
protocol ReturnTypes {
    function getU64() -> u64;
    function getStr() -> str;
    function getBool() -> bool;
    function getArray() -> str[];
    function noReturn(u64);
}
"#;
        let parsed = grammar::parse(code);
        assert!(parsed.is_ok());

        let ir_units = IncrementalInterpreter::from_source(code);
        assert_eq!(ir_units.len(), 1);
        match &ir_units[0] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Protocol { functions, .. } => {
                assert_eq!(functions.len(), 5);
            }
            _ => panic!("Expected Protocol"),
        }
    }

    #[test]
    fn test_const_primitive_values() {
        let code = r#"
const U8_VAL: u8 = 255
const U16_VAL: u16 = 65535
const U32_VAL: u32 = 4294967295
const I8_MIN: s8 = -128
const I8_MAX: s8 = 127
const BOOL_TRUE: bool = true
const BOOL_FALSE: bool = false
const STR_VAL: str = "hello"
"#;
        let parsed = grammar::parse(code);
        assert!(parsed.is_ok());

        let ir_units = IncrementalInterpreter::from_source(code);
        assert_eq!(ir_units.len(), 8); // 8 constants
        match &ir_units[0] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Constant { name, .. } => {
                assert_eq!(name, "U8_VAL");
            }
            _ => panic!("Expected Constant"),
        }
    }

    #[test]
    fn test_nested_custom_types() {
        let code = r#"
struct Inner {
    value: u64
}

struct Outer {
    inner: Inner
    items: Inner[]
}

protocol Service {
    function get() -> Outer;
    function process(Outer) -> bool;
}
"#;
        let parsed = grammar::parse(code);
        assert!(parsed.is_ok());

        let ir_units = IncrementalInterpreter::from_source(code);
        assert_eq!(ir_units.len(), 3); // inner, outer, service
        match &ir_units[1] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Struct { name, fields, .. } => {
                assert_eq!(name, "Outer");
                assert_eq!(fields.len(), 2);
            }
            _ => panic!("Expected Outer Struct"),
        }
    }

    #[test]
    fn test_mixed_array_types() {
        let code = r#"
struct ArrayTest {
    dynamic: str[]
    fixed: u8[256]
    nested: u32[][]
    custom_array: Inner[]
    fixed_custom: Inner[10]
}

struct Inner {
    id: u64
}
"#;
        let parsed = grammar::parse(code);
        assert!(parsed.is_ok());

        let ir_units = IncrementalInterpreter::from_source(code);
        assert_eq!(ir_units.len(), 2); // Struct + Inner Struct
        match &ir_units[0] {
            comline_core::schema::ir::frozen::unit::FrozenUnit::Struct { name, fields, .. } => {
                assert_eq!(name, "ArrayTest");
                assert_eq!(fields.len(), 5);
            }
            _ => panic!("Expected ArrayTest Struct"),
        }
    }

    #[test]
    fn test_all_declaration_types_together() {
        let code = r#"
import std

const VERSION: str = "1.0"
const MAX: u32 = 1000

enum Status {
    Active
    Inactive
}

struct Data {
    id: u64
    status: Status
}

protocol API {
    function get(u64) -> Data;
}
"#;
        let parsed = grammar::parse(code);
        assert!(parsed.is_ok());

        let ir_units = IncrementalInterpreter::from_source(code);
        // import + 2 consts + enum + struct + protocol = 6 units
        assert_eq!(ir_units.len(), 6);
    }

    #[test]
    fn test_complex_real_world_schema() {
        let code = r#"
import std

const API_VERSION: str = "2.0"
const MAX_USERS: u32 = 10000
const TIMEOUT_MS: s32 = 5000

enum UserRole {
    Admin
    User
    Guest
}

enum MessageType {
    Text
    Image
    Video
    File
}

struct Address {
    street: str
    city: str
    country: str
}

struct User {
    id: u64
    username: str
    email: str
    role: UserRole
    address: Address
    tags: str[]
}

struct Message {
    id: u64
    sender_id: u64
    type: MessageType
    content: str
    timestamp: u64
}

struct Conversation {
    id: u64
    participants: u64[]
    messages: Message[]
}

protocol UserService {
    function createUser(str, str, UserRole) -> u64;
    function getUser(u64) -> User;
    function updateUser(u64, str) -> bool;
    function deleteUser(u64) -> bool;
    function listUsers(u32, u32) -> User[];
}

protocol MessagingService {
    function sendMessage(u64, u64, MessageType, str) -> u64;
    function getConversation(u64) -> Conversation;
    function markAsRead(u64) -> bool;
}
"#;
        let parsed = grammar::parse(code);
        assert!(parsed.is_ok());

        let ir_units = IncrementalInterpreter::from_source(code);
        // import + 3 consts + 2 enums + 4 structs + 2 protocols = 12 units
        assert_eq!(ir_units.len(), 12);
    }

    // ===== TYPE ALIAS DIAGNOSTICS, VIA THE PUBLIC `check_and_from_declarations` ENTRY =====
    //
    // `compiler::alias_resolution`'s own unit tests already cover
    // `check_aliases`'s internals directly; these confirm the public
    // `IncrementalInterpreter::check_and_from_declarations` wiring itself
    // reports errors (and, on success, still returns correctly-substituted
    // units) without requiring a full `ProjectContext`.

    fn declarations_for(code: &str) -> Vec<rust_sitter::Spanned<comline_core::schema::idl::grammar::Declaration>> {
        grammar::parse(code).expect("should parse").0
    }

    #[test]
    fn test_check_and_from_declarations_rejects_a_cycle() {
        let declarations = declarations_for("type A = B\ntype B = A\n");
        let result = IncrementalInterpreter::check_and_from_declarations(declarations, None);
        assert!(result.is_err(), "a cyclic alias should be rejected");
    }

    #[test]
    fn test_check_and_from_declarations_rejects_an_unresolvable_target() {
        let declarations = declarations_for(
            "type X = Nope\nstruct A {\n    a: X\n}\nstruct B {\n    b: X\n}\n",
        );
        let result = IncrementalInterpreter::check_and_from_declarations(declarations, None);
        let errors = result.expect_err("an unresolvable alias target should be rejected");
        assert_eq!(errors.len(), 1, "one error at the alias, not one per use site");
    }

    #[test]
    fn test_check_and_from_declarations_succeeds_and_freezes_the_alias() {
        use comline_core::schema::ir::compiler::interpreted::kind_search::KindValue;
        use comline_core::schema::ir::frozen::unit::FrozenUnit;

        let declarations = declarations_for("type UserId = u64\nstruct X {\n    id: UserId\n}\n");
        let ir_units = IncrementalInterpreter::check_and_from_declarations(declarations, None)
            .expect("a valid alias should compile");

        assert_eq!(ir_units.len(), 2, "the alias freezes to its own unit, alongside the struct");
        assert!(ir_units.iter().any(|u| matches!(
            u,
            FrozenUnit::TypeAlias { name, target, .. }
                if name == "UserId" && *target == KindValue::Namespaced("u64".to_string(), None)
        )));
        let field_kind = ir_units
            .iter()
            .find_map(|u| match u {
                FrozenUnit::Struct { fields, .. } => match &fields[0] {
                    FrozenUnit::Field { kind_value, .. } => Some(kind_value),
                    _ => None,
                },
                _ => None,
            })
            .expect("a struct with one field");
        assert_eq!(
            field_kind,
            &KindValue::Namespaced("UserId".to_string(), None),
            "the field keeps the alias's name, same as a struct/enum reference would"
        );
    }
}

/// A duplicate declaration is a two-site problem. Before this, the
/// diagnostic named one site, dumped its kind with `{:?}`, and - when the
/// offending declaration was an `error`, which carried no span at all -
/// pointed at byte 0, i.e. the top of the file.
#[cfg(test)]
mod duplicate_declaration_diagnostics {
    use comline_core::diagnostics::render;
    use comline_core::schema::ir::compiler::interpreter::incremental::IncrementalInterpreter;
    use comline_core::schema::ir::compiler::Compile;
    use comline_core::schema::ir::frozen::unit::FrozenUnit;
    use comline_core::schema::ir::validation::validate;

    /// The byte range `source` quotes for `span`.
    fn quoted(source: &str, span: (usize, usize)) -> &str {
        &source[span.0..span.1]
    }

    #[test]
    fn a_struct_and_an_error_name_both_sites() {
        let code = "struct Test {\n    a: str\n}\n\nerror Test {\n    message = \"\"\n}\n";
        let errors = validate(&IncrementalInterpreter::from_source(code)).unwrap_err();

        assert_eq!(errors.len(), 1);
        let error = &errors[0];
        assert_eq!(error.code.id, "validation::duplicate-declaration");

        // Primary: the `error` that collided - not byte 0.
        let span = error.span.expect("the error declaration has a span now");
        assert!(quoted(code, span).starts_with("error Test"), "{:?}", quoted(code, span));

        // Secondary: the struct already holding the name.
        let (sec_span, sec_label) = error.secondary.as_ref().expect("both sites are named");
        assert!(quoted(code, *sec_span).starts_with("struct Test"));
        assert_eq!(sec_label, "struct 'Test' declared here");

        assert_eq!(error.context, "error 'Test'");
        assert_eq!(
            error.help.as_deref(),
            Some("rename this error or remove it - the struct above already defines 'Test'"),
        );

        // Both sites really do render, not just travel in the struct.
        let rendered = render(error, "test.ids", code);
        assert!(rendered.contains("struct Test"), "{rendered}");
        assert!(rendered.contains("error Test"), "{rendered}");
    }

    /// Declaring the same `error` twice was caught by `core#89`, but the
    /// report had nowhere to point.
    #[test]
    fn two_errors_name_both_sites() {
        let code = "error Test {\n    message = \"a\"\n}\n\nerror Test {\n    message = \"b\"\n}\n";
        let errors = validate(&IncrementalInterpreter::from_source(code)).unwrap_err();

        assert_eq!(errors.len(), 1);
        let span = errors[0].span.expect("spanned");
        let (sec_span, _) = errors[0].secondary.as_ref().expect("two sites");
        assert_ne!(span, *sec_span, "the two errors are different declarations");
        assert!(quoted(code, span).contains("\"b\""), "the primary is the second one");
        assert!(quoted(code, *sec_span).contains("\"a\""), "the secondary is the first");
    }

    /// A `validator` had the same spanless gap an `error` did.
    #[test]
    fn a_validator_colliding_with_a_struct_is_spanned() {
        let code = concat!(
            "struct Bounded {\n    a: str\n}\n\n",
            "validator Bounded {\n    min: u32 = 1\n\n    validate {\n",
            "        assert(value.length >= params.min, \"too short\")\n    }\n}\n",
        );
        let errors = validate(&IncrementalInterpreter::from_source(code)).unwrap_err();

        assert_eq!(errors.len(), 1);
        let span = errors[0].span.expect("the validator declaration has a span now");
        assert!(quoted(code, span).starts_with("validator Bounded"));
        assert_eq!(errors[0].context, "validator 'Bounded'");
    }

    /// `settings` presets are their own namespace - a `settings Test`
    /// beside a `struct Test` generates nothing and collides with nothing.
    #[test]
    fn a_settings_preset_may_share_a_name() {
        let code = "struct Test {\n    a: str\n}\n\nsettings Test {\n    validators.allowed = true\n}\n";
        validate(&IncrementalInterpreter::from_source(code))
            .expect("a settings preset shares no namespace with a struct");
    }

    /// With a namespace in hand - what a real `comline build` always has -
    /// both sites are named the way the author would have to write them
    /// from another schema.
    #[test]
    fn a_namespace_qualifies_both_sites() {
        let code = "struct Test {\n    a: str\n}\n\nerror Test {\n    message = \"\"\n}\n";
        let mut units = IncrementalInterpreter::from_source(code);
        units.insert(0, FrozenUnit::Namespace("test::main".to_string()));

        let errors = validate(&units).unwrap_err();
        assert_eq!(errors[0].message, "Duplicate definition of 'test::main::Test'");
        assert_eq!(errors[0].context, "error 'test::main::Test'");
        let (_, sec_label) = errors[0].secondary.as_ref().expect("two sites");
        assert_eq!(sec_label, "struct 'test::main::Test' declared here");
        // The help names the bare identifier - it's telling you what to type.
        assert!(errors[0].help.as_deref().unwrap().ends_with("already defines 'Test'"));
    }
}
