// Standard Uses
use std::path::Path;

// Local Uses

// External Uses
use comline_core::schema::{idl, ir};
use comline_core::schema::idl::constants::SCHEMA_EXTENSION;


#[allow(unused)]
#[test]
fn from_raw_to_unit() {
    let path = &format!("tests/idl/simple.{}", SCHEMA_EXTENSION);
    let path = Path::new(path);
    let raw = std::fs::read_to_string(path).unwrap();
    let sourced = idl::parser_new::parse_source(
        raw, path.to_str().unwrap().to_owned()
    ).unwrap();

    /*
    pretty_assertions::assert_eq!(
        sourced.1, vec![
            (Span(1), ASTUnit::Namespace(Span(2), "tests::idl::simple".to_owned())),
            (Span(3), (ASTUnit::Import(
                Span(4), "std::validators::string_bounds::StringBounds".to_string()))
            ),
            (Span(5), ASTUnit::Constant {
                docstring: vec![],
                name: (Span(6), "POWER".to_owned()),
                kind: (Span(7), "u8".to_owned()),
                default_value: Some((Span(8), "1".to_owned())),
            }),
            (Span(9), ASTUnit::Constant {
                docstring: vec![],
                name: (Span(10), "DEFAULT_NAME".to_owned()),
                kind: (Span(11), "str".to_owned()),
                default_value: Some((Span(12), "f\"flower power: {POWER}\"".to_owned())),
            }),
            (Span(13), ASTUnit::Settings {
                docstring: vec![],
                name: (Span(14), "Test".to_owned()),
                parameters: vec![
                    (Span(15), ASTUnit::Parameter {
                        name: (Span(16), "forbid_indexing".to_string()),
                        default_value: (Span(17), "True".to_string()),
                    }),
                    (Span(18), ASTUnit::Parameter {
                        name: (Span(19), "forbid_optional_indexing".to_string()),
                        default_value: (Span(20), "True".to_string()),
                    })
                ],
            }),
            (Span(21), ASTUnit::Enum {
                docstring: vec![],
                name: (Span(22), "EncryptionAlgorithm".to_owned()),
                variants: vec![
                    (Span(23), ASTUnit::EnumVariant {
                        name: (Span(24), "Bad".to_owned()), kind: None
                    }),
                    (Span(25), ASTUnit::EnumVariant {
                        name: (Span(26), "Medium".to_owned()), kind: None
                    })
                ]
            }),
            (Span(27), ASTUnit::Enum {
                docstring: vec![],
                name: (Span(28), "EncryptionMode".to_string()),
                variants: vec![
                    (Span(29), ASTUnit::EnumVariant {
                        name: (Span(30), "None".to_string()), kind: None
                    }),
                    (Span(31), ASTUnit::EnumVariant {
                        name: (Span(32), "Encrypt".to_string()), kind: None
                    })
                ],
            }),
            (Span(33), ASTUnit::Struct {
                docstring: vec![
                    (Span(34), ASTUnit::Docstring {
                        variable: None,
                        description: " A message that can be sent through the mail protocol".to_owned(),
                    }),
                ],
                parameters: vec![],
                name: (Span(35), "Message".to_owned()),
                fields: vec![
                    (Span(36), ASTUnit::Field {
                        docstring: vec![],
                        parameters: vec![],
                        optional: false,
                        name: "name".to_owned(),
                        kind: "str".to_owned(),
                        default_value: Some("DEFAULT_NAME".to_owned()),
                    }),
                    (Span(37), ASTUnit::Field {
                        docstring: vec![],
                        parameters: vec![],
                        optional: false,
                        name: "encryption_mode".to_owned(),
                        kind: "EncryptionMode".to_owned(),
                        default_value: Some("default".to_owned()),
                    }),
                    (Span(38), ASTUnit::Field {
                        docstring: vec![],
                        parameters: vec![],
                        optional: true,
                        name: "recipient".to_owned(),
                        kind: "str".to_owned(),
                        default_value: Some("\"bee\"".to_owned()),
                    })
                ],
            }),
            (Span(39), ASTUnit::Error {
                docstring: vec![
                    (Span(40), ASTUnit::Docstring { variable: None,
                        description: " Throw when sending a message to a missing recipient".to_string() }
                    ),
                ],
                parameters: vec![
                    (Span(42), ASTUnit::Parameter {
                        name: (Span(43), "message".to_owned()),
                        default_value: (
                            Span(44),
                            "\"Recipient with name {self.recipient} not found\"".to_owned()
                        ),
                    })
                ],
                name: (Span(41), "RecipientNotFoundError".to_owned()),
                properties: vec![],
                fields: vec![
                    (Span(45), ASTUnit::Field {
                        docstring: vec![],
                        parameters: vec![],
                        optional: false,
                        name: "recipient".to_string(),
                        kind: "str".to_string(),
                        default_value: None,
                    })
                ],
            }),
            (Span(46), ASTUnit::Protocol {
                docstring: vec![
                    (Span(47), ASTUnit::Docstring {
                        variable: None,
                        description: " Mail API for receiving and sending emails".to_string(),
                    })
                ],
                parameters: vec![
                    (Span(48), ASTUnit::Parameter {
                        name: (Span(49), "provider".to_owned()),
                        default_value: (Span(50), "Any".to_owned(), ),
                    }),
                ],
                name: (Span(51), "Mail".to_owned()),
                functions: vec![
                    (Span(52), ASTUnit::Function {
                        docstring: vec![],
                        parameters: vec![
                            (Span(53), ASTUnit::Parameter {
                                name: (Span(54), "timeout_ms".to_string()),
                                default_value: (Span(55), "1000".to_string()),
                            })
                        ],
                        name: (Span(56), "send_message".to_owned()),
                        asynchronous: None,
                        arguments: vec![
                            (Span(57), ASTUnit::Argument {
                                name: Some((Span(58), "message".to_owned())),
                                kind: (Span(59), "Message".to_string()),
                            })
                        ],
                        returns: vec![
                            (Span(60), "str".to_owned())
                        ],
                        throws: vec![
                            (Span(61), "RecipientNotFoundError(function.message.recipient)\n".to_owned())
                        ],
                    })
                ],
            })
        ]
    )
    */
}


#[test]
fn compile_unit() {
    let path = &format!("tests/idl/simple.{}", SCHEMA_EXTENSION);
    let path = Path::new(path);
    let unit = idl::parser_new::from_path(path).unwrap();

    let context = ir::context::SchemaContext::with_ast(
        unit, vec![path.file_stem().unwrap().to_str().unwrap().to_owned()]
    );

    /*
    let frozen_unit = interpreter::interpret_context(context).unwrap();

    pretty_assertions::assert_eq!(
        frozen_unit, vec![
            FrozenUnit::Namespace("tests::idl::simple".to_string()),
            FrozenUnit::Import("std::validators::string_bounds::StringBounds".to_string()),
            FrozenUnit::Constant {
                docstring: None,
                name: "POWER".to_string(),
                kind_value: KindValue::Primitive(Primitive::U8(Some(1))),
            },
            FrozenUnit::Constant {
                docstring: None,
                name: "DEFAULT_NAME".to_string(),
                kind_value: KindValue::Primitive(Primitive::String(
                    Some("f\"flower power: {POWER}\"".to_string())
                )),
            },
            FrozenUnit::Enum {
                docstring: None,
                name: "EncryptionAlgorithm".to_string(), variants: vec![
                    FrozenUnit::EnumVariant(KindValue::EnumVariant(
                        "Bad".to_owned(), None
                    )),
                    FrozenUnit::EnumVariant(KindValue::EnumVariant(
                        "Medium".to_owned(), None
                    ))
                ],
            },
            FrozenUnit::Enum {
                docstring: None,
                name: "EncryptionMode".to_string(), variants: vec![
                    FrozenUnit::EnumVariant(KindValue::EnumVariant(
                        "None".to_owned(), None)
                    ),
                    FrozenUnit::EnumVariant(KindValue::EnumVariant(
                        "Encrypt".to_owned(),
                        None
                        // TODO; The reference bellow is a feature that needs thought, uncomment later
                        /*
                        Some(Box::from(KindValue::EnumVariant(
                            "Medium".to_string(), None
                        )))
                        */
                    ))
                ]
            }
        ]
    );
    */
}
