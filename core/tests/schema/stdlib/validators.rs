// Standard Uses
use std::path::Path;

// Local Uses

// External Uses
use once_cell::sync::Lazy;
use comline::schema::{idl, ir};
use comline::schema::idl::ast::unit::{ASTUnit, SourcedWhole};
use comline::utils::codemap::{CodeMap, Span};


static STRING_BOUNDS_SCHEMA: &str = "src/lang_lib/validators/string_bounds.ids";
static STRING_BOUNDS_SCHEMA_PATH: Lazy<&Path> = Lazy::new(||
    Path::new(STRING_BOUNDS_SCHEMA)
);

#[test]
fn parse_string_bounds_unit() {
    let unit = idl::parser_new::from_path(&STRING_BOUNDS_SCHEMA_PATH).unwrap();

    pretty_assertions::assert_eq!(
        unit.1, expected_string_bounds_validator_ast(STRING_BOUNDS_SCHEMA).1
    )
}

#[test]
fn parse_string_bounds_whole_unit() {
    let unit = idl::parser_new::from_path(&STRING_BOUNDS_SCHEMA_PATH).unwrap();

    let context = ir::context::SchemaContext::with_main(unit);

    pretty_assertions::assert_eq!(
        context.schema.1, expected_string_bounds_validator_ast(STRING_BOUNDS_SCHEMA).1
    )

    /*
    let interpreted = compiler::interpret_unit(context);

    println!("{:?}", interpreted);
    */
}

#[allow(unused)]
#[test]
fn compile_string_bounds_unit() {
    let unit = idl::parser_new::from_path(&STRING_BOUNDS_SCHEMA_PATH).unwrap();

    let context = ir::context::SchemaContext::with_main_no_std(unit);

    /*
    let frozen_unit = interpreter::interpret_unit(context).unwrap();

    pretty_assertions::assert_eq!(
        frozen_unit, vec![
            FrozenUnit::Namespace("std::validators::string_bounds".to_owned()),
        ]
    );
    */
}

#[allow(unused)]
fn expected_string_bounds_validator_ast(path: &str) -> SourcedWhole {
    (
        CodeMap::new(),
        vec![
            (Span(1), ASTUnit::Namespace(
                Span(2), "std::validators::string_bounds".to_owned())
            ),
            (Span(3), ASTUnit::Validator {
                docstring: vec![
                    (Span(1), ASTUnit::Docstring {
                        variable: None,
                        description:
                        " Checks if a string length is between a minimum and a maximum".to_owned(),
                    }),
                    (Span(1), ASTUnit::Docstring {
                        variable: Some("min_chars".to_owned()),
                        description: "Minimum length of the string".to_owned(),
                    }),
                    (Span(1), ASTUnit::Docstring {
                        variable: Some("max_chars".to_owned()),
                        description: "Maximum length of the string".to_owned(),
                    }),
                ],
                properties: vec![
                    (Span(1), ASTUnit::Property {
                        name: (Span(1), "min_chars".to_string()), expression: None,
                    }),
                    (Span(1), ASTUnit::Property {
                        name: (Span(1), "max_chars".to_string()), expression: None,
                    })
                ],
                name: (Span(1), "StringBounds".to_string()),
                expression_block: Box::new(
                    (Span(1), ASTUnit::ExpressionBlock { function_calls: vec![] })
                ),
            }),
        ],
    )
}
