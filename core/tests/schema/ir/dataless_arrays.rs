// `T[]` of a type that carries no data is refused: packed@1 writes the
// count and nothing per element, and its decoder refuses a count larger
// than the bytes left, so such an array can't be decoded.

use comline_core::diagnostics::Diagnostic;
use comline_core::package::config::idl::grammar as config_grammar;
use comline_core::package::config::ir::compiler::interpret::interpret_context;
use comline_core::package::config::ir::context::ProjectContext;
use comline_core::schema::idl::grammar;
use comline_core::schema::ir::compiler::interpreter::incremental::IncrementalInterpreter;
use comline_core::schema::ir::compiler::Compile;
use comline_core::schema::ir::context::SchemaContext;
use comline_core::schema::ir::validation::validate;
use comline_core::utils::codemap::CodeMap;

use std::cell::RefCell;
use std::rc::Rc;

const CODE: &str = "validation::array-of-dataless-type";

/// The data-less-array errors validating `code` gives.
fn refused(code: &str) -> Vec<Diagnostic> {
    grammar::parse(code).expect("the schema parses");
    validate(&IncrementalInterpreter::from_source(code))
        .err()
        .unwrap_or_default()
        .into_iter()
        .filter(|e| e.code.id == CODE)
        .collect()
}

fn messages(code: &str) -> Vec<String> {
    refused(code).into_iter().map(|e| e.message).collect()
}

#[test]
fn an_array_of_an_empty_struct_is_refused() {
    let errors = refused("struct Empty {}\n\nstruct Batch {\n    items: Empty[]\n}\n");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(
        errors[0].message,
        "an array of `Empty` carries only its length: `Empty` has no data, so each element \
         would take no bytes"
    );
    assert_eq!(
        errors[0].help.as_deref(),
        Some("give `Empty` a field, or use a count (`u32`) instead")
    );
    assert_eq!(errors[0].context, "Struct 'Batch', field 'items'");
}

#[test]
fn an_array_of_unit_is_refused() {
    let code = "protocol P {\n    function acks() -> ()[];\n}\n";
    assert_eq!(
        messages(code),
        ["an array of `()` carries only its length: `()` has no data, so each element would \
          take no bytes"]
    );
    assert_eq!(refused(code)[0].help.as_deref(), Some("use a count (`u32`) instead"));
}

#[test]
fn an_array_of_an_alias_of_an_empty_struct_is_refused() {
    let code = "struct Empty {}\ntype Nothing = Empty\ntype Still = Nothing\n\n\
                struct Batch {\n    items: Still[]\n}\n";
    assert_eq!(messages(code).len(), 1, "{:?}", messages(code));
    assert!(messages(code)[0].starts_with("an array of `Still` carries only its length"));
}

#[test]
fn an_array_of_a_struct_of_empty_structs_is_refused() {
    let code = "struct Empty {}\nstruct Pair {\n    a: Empty\n    b: Empty[4]\n}\n\n\
                struct Batch {\n    items: Pair[]\n}\n";
    assert_eq!(messages(code).len(), 1);
}

#[test]
fn an_array_of_a_zero_length_array_is_refused() {
    let code = "struct Batch {\n    items: u64[0][]\n}\n";
    assert_eq!(
        messages(code),
        ["an array of `u64[0]` carries only its length: `u64[0]` has no data, so each element \
          would take no bytes"]
    );
}

#[test]
fn every_place_a_type_is_written_is_checked() {
    let code = "struct Empty {}\n\
                error Failed {\n    message = \"failed\"\n    causes: Empty[]\n}\n\
                type Many = Empty[]\n\
                struct Holder {\n    nested: Empty[][]\n    either: union(Empty[] u8)\n}\n\
                protocol P {\n    function put(Empty[]) -> Empty[];\n}\n";
    let contexts: Vec<String> = refused(code).into_iter().map(|e| e.context).collect();
    assert_eq!(
        contexts,
        [
            "Error 'Failed', field 'causes'",
            "Type alias 'Many'",
            "Struct 'Holder', field 'nested'",
            "Struct 'Holder', field 'either'",
            "Protocol 'P', function 'put', arg 'arg0'",
            "Protocol 'P', function 'put' return",
        ]
    );
}

#[test]
fn a_self_referential_struct_is_not_refused() {
    let code = "struct Node {\n    children: Node[]\n}\n\n\
                struct Chain {\n    optional next: Chain\n}\n\n\
                struct Forest {\n    trees: Node[]\n    chains: Chain[]\n}\n";
    assert!(messages(code).is_empty(), "{:?}", messages(code));
}

#[test]
fn a_fixed_array_of_an_empty_struct_and_an_optional_field_are_allowed() {
    let code = "struct Empty {}\n\
                struct Maybe {\n    optional nothing: Empty\n}\n\
                struct Batch {\n    three: Empty[3]\n    maybes: Maybe[]\n    empty: Empty\n}\n";
    assert!(messages(code).is_empty(), "{:?}", messages(code));
}

#[test]
fn the_span_points_at_the_array_type() {
    // An argument's span is its type.
    let code = "struct Empty {}\n\nprotocol P {\n    function put(Empty[]);\n}\n";
    let at = code.find("Empty[]").unwrap();
    assert_eq!(refused(code)[0].span, Some((at, at + "Empty[]".len())));

    // A field's is the field, as every other type check reports it.
    let code = "struct Empty {}\n\nstruct Batch {\n    items: Empty[]\n}\n";
    let at = code.find("items").unwrap();
    assert_eq!(refused(code)[0].span, Some((at, at + "items: Empty[]".len())));
}

fn add_schema(project: &mut ProjectContext, namespace: &[&str], source: &str) {
    let document = grammar::parse(source).expect("schema should parse");
    let mut codemap = CodeMap::new();
    codemap.insert_file(namespace.join("/"), source.to_string());
    let namespace: Vec<String> = namespace.iter().map(|s| s.to_string()).collect();
    let context = SchemaContext::with_declarations(document.0, namespace, codemap);
    project.schema_contexts.push(Rc::new(RefCell::new(context)));
}

fn project() -> ProjectContext {
    let congregation = config_grammar::parse("congregation test\nspecification_version = 1")
        .expect("congregation should parse");
    ProjectContext::with_config(congregation)
}

#[test]
fn an_imported_data_less_type_is_refused_through_aliases_and_schemas() {
    let mut project = project();
    add_schema(&mut project, &["empty"], "struct Empty {}\n");
    add_schema(&mut project, &["types"], "use empty::Empty\n\ntype Nothing = Empty\n");
    add_schema(&mut project, &["api"], "use types::Nothing\n\nstruct Batch {\n    items: Nothing[]\n}\n");

    let error = interpret_context(&project).expect_err("the array is refused").to_string();
    assert!(error.contains("an array of `Nothing` carries only its length"), "{error}");
}

#[test]
fn an_imported_type_with_data_is_not_refused() {
    let mut project = project();
    add_schema(&mut project, &["types"], "struct Node {\n    children: Node[]\n}\n\nstruct Flag {\n    optional set: bool\n}\n");
    add_schema(
        &mut project,
        &["api"],
        "use types::Node\nuse types::Flag\n\nstruct Batch {\n    nodes: Node[]\n    flags: Flag[]\n}\n",
    );

    interpret_context(&project).expect("arrays of types that carry data build");
}
