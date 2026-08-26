// Cross-file `use` resolution tests: exercise the real compile path
// (ProjectContext + interpret_context), not the single-file from_source path,
// since only that path is aware of other schemas in the project.

use comline_core::package::config::idl::grammar as config_grammar;
use comline_core::package::config::ir::compiler::interpret::interpret_context;
use comline_core::package::config::ir::context::ProjectContext;
use comline_core::schema::idl::grammar;
use comline_core::schema::ir::context::SchemaContext;
use comline_core::schema::ir::frozen::unit::FrozenUnit;
use comline_core::utils::codemap::CodeMap;

use std::cell::RefCell;
use std::rc::Rc;

fn build_project() -> ProjectContext {
    let congregation = config_grammar::parse("congregation test\nspecification_version = 1")
        .expect("congregation should parse");
    ProjectContext::with_config(congregation)
}

fn add_schema(project: &mut ProjectContext, namespace: &[&str], source: &str) {
    let document = grammar::parse(source).expect("schema should parse");

    let mut codemap = CodeMap::new();
    codemap.insert_file(namespace.join("/"), source.to_string());

    let namespace: Vec<String> = namespace.iter().map(|s| s.to_string()).collect();
    let context = SchemaContext::with_declarations(document.0, namespace, codemap);

    project.schema_contexts.push(Rc::new(RefCell::new(context)));
}

fn frozen_units_for<'a>(project: &'a ProjectContext, namespace: &str) -> Vec<FrozenUnit> {
    project
        .schema_contexts
        .iter()
        .find(|schema| schema.borrow().namespace_joined() == namespace)
        .expect("schema should exist")
        .borrow()
        .frozen_schema
        .borrow()
        .clone()
        .expect("schema should be frozen")
}

#[test]
fn test_whole_schema_use_resolves_across_files() {
    let mut project = build_project();
    add_schema(&mut project, &["types"], "struct User {\n    id: u64\n}\n");
    add_schema(
        &mut project,
        &["api"],
        "use types\n\nstruct Response {\n    id: u64\n}\n",
    );

    interpret_context(&project).expect("compilation should succeed");

    let frozen = frozen_units_for(&project, "api");
    assert!(
        frozen
            .iter()
            .any(|unit| matches!(unit, FrozenUnit::Import(path, _) if path == "types")),
        "Expected a resolved import of 'types', got {:?}",
        frozen
    );
}

#[test]
fn test_symbol_use_resolves_to_declaring_schema() {
    let mut project = build_project();
    add_schema(&mut project, &["types"], "struct User {\n    id: u64\n}\n");
    add_schema(
        &mut project,
        &["api"],
        "use types::User\n\nstruct Response {\n    id: u64\n}\n",
    );

    interpret_context(&project).expect("compilation should succeed");

    let frozen = frozen_units_for(&project, "api");
    assert!(
        frozen
            .iter()
            .any(|unit| matches!(unit, FrozenUnit::Import(path, _) if path == "types::User")),
        "Expected a resolved import of 'types::User', got {:?}",
        frozen
    );
}

#[test]
fn test_multi_item_use_resolves_each_symbol() {
    let mut project = build_project();
    add_schema(
        &mut project,
        &["types"],
        "struct User {\n    id: u64\n}\n\nstruct Post {\n    id: u64\n}\n",
    );
    add_schema(
        &mut project,
        &["api"],
        "use types::{User, Post}\n\nstruct Response {\n    id: u64\n}\n",
    );

    interpret_context(&project).expect("compilation should succeed");

    let frozen = frozen_units_for(&project, "api");
    for expected in ["types::User", "types::Post"] {
        assert!(
            frozen
                .iter()
                .any(|unit| matches!(unit, FrozenUnit::Import(path, _) if path == expected)),
            "Expected a resolved import of '{}', got {:?}",
            expected,
            frozen
        );
    }
}

#[test]
fn test_glob_use_resolves_namespace() {
    let mut project = build_project();
    add_schema(&mut project, &["types"], "struct User {\n    id: u64\n}\n");
    add_schema(
        &mut project,
        &["api"],
        "use types::*\n\nstruct Response {\n    id: u64\n}\n",
    );

    interpret_context(&project).expect("compilation should succeed");

    let frozen = frozen_units_for(&project, "api");
    assert!(
        frozen
            .iter()
            .any(|unit| matches!(unit, FrozenUnit::Import(path, _) if path == "types::*")),
        "Expected a resolved glob import of 'types::*', got {:?}",
        frozen
    );
}

#[test]
fn test_import_cycle_between_schemas_is_rejected() {
    let mut project = build_project();
    add_schema(&mut project, &["a"], "use b\n\nstruct A {\n    id: u64\n}\n");
    add_schema(&mut project, &["b"], "use a\n\nstruct B {\n    id: u64\n}\n");

    let result = interpret_context(&project);
    assert!(result.is_err(), "Expected a cycle error, got {:?}", result);
}

#[test]
fn test_unresolved_use_does_not_panic() {
    let mut project = build_project();
    add_schema(
        &mut project,
        &["api"],
        "use std::http::Request\n\nstruct Response {\n    id: u64\n}\n",
    );

    // Not part of this project and stdlib isn't configured - should not panic,
    // just fall back to an explicit unresolved-import marker.
    interpret_context(&project).expect("compilation should succeed");

    let frozen = frozen_units_for(&project, "api");
    assert!(
        frozen
            .iter()
            .any(|unit| matches!(unit, FrozenUnit::Import(path, _) if path.starts_with("<unresolved:"))),
        "Expected an unresolved-import marker, got {:?}",
        frozen
    );
}

#[test]
fn test_same_package_symbol_not_found_still_compiles() {
    let mut project = build_project();
    add_schema(&mut project, &["types"], "struct User {\n    id: u64\n}\n");
    add_schema(
        &mut project,
        &["api"],
        "use types::Missing\n\nstruct Response {\n    id: u64\n}\n",
    );

    // The symbol doesn't exist in the target schema - this should still
    // compile (best-effort) rather than panic; catching it is validation's job.
    interpret_context(&project).expect("compilation should succeed");

    let frozen = frozen_units_for(&project, "api");
    assert!(
        frozen
            .iter()
            .any(|unit| matches!(unit, FrozenUnit::Import(path, _) if path == "types::Missing")),
        "Expected a best-effort import of 'types::Missing', got {:?}",
        frozen
    );
}
