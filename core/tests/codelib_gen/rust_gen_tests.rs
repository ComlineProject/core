use comline_core::codelib_gen::rust::generate_rust;
use comline_core::schema::ir::frozen::unit::{FrozenUnit, FrozenArgument};
use comline_core::schema::ir::compiler::interpreted::kind_search::{KindValue, Primitive};

#[test]
fn test_generate_simple_struct() {
    let units = vec![
        FrozenUnit::Struct {
            docstring: None,
            parameters: vec![],
            name: "User".to_string(),
            fields: vec![
                FrozenUnit::Field {
                    docstring: None,
                    parameters: vec![],
                    optional: false,
                    name: "id".to_string(),
                    kind_value: KindValue::Namespaced("i32".to_string(), None),
                },
                FrozenUnit::Field {
                    docstring: None,
                    parameters: vec![],
                    optional: false,
                    name: "username".to_string(),
                    kind_value: KindValue::Namespaced("string".to_string(), None),
                },
                FrozenUnit::Field {
                    docstring: None,
                    parameters: vec![],
                    optional: false,
                    name: "tags".to_string(),
                    kind_value: KindValue::Namespaced("string[]".to_string(), None),
                }
            ],
        }
    ];

    let output = generate_rust(&units);
    
    assert!(output.contains("pub struct User"));
    assert!(output.contains("pub id: i32"));
    assert!(output.contains("pub username: String"));
    assert!(output.contains("pub tags: Vec<String>"));
}

#[test]
fn test_generate_enum() {
    let units = vec![
        FrozenUnit::Enum {
            docstring: None,
            name: "Status".to_string(),
            variants: vec![
                FrozenUnit::EnumVariant(KindValue::EnumVariant("Active".to_string(), None)),
                FrozenUnit::EnumVariant(KindValue::EnumVariant("Inactive".to_string(), None)),
            ],
        }
    ];

    let output = generate_rust(&units);
    
    assert!(output.contains("pub enum Status"));
    assert!(output.contains("Active,"));
    assert!(output.contains("Inactive,"));
}

#[test]
fn test_generate_protocol() {
    let units = vec![
        FrozenUnit::Protocol {
            docstring: "A user service".to_string(),
            name: "UserService".to_string(),
            parameters: vec![],
            functions: vec![
                FrozenUnit::Function {
                    docstring: "".to_string(),
                    name: "get_user".to_string(),
                    synchronous: true,
                    arguments: vec![
                        FrozenArgument {
                            name: "id".to_string(),
                            kind: KindValue::Primitive(Primitive::S32(None)),
                        }
                    ],
                    _return: Some(KindValue::Namespaced("User".to_string(), None)),
                    throws: vec![],
                }
            ],
        }
    ];

    let output = generate_rust(&units);
    
    assert!(output.contains("pub trait UserService"));
    assert!(output.contains("fn get_user(id: i32) -> User;"));
}
