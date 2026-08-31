//! `PackageSources` — compiling a package from in-memory strings, no filesystem.

use comline_core::package::build::PackageSources;

#[test]
fn compiles_a_single_schema_with_a_synthesised_congregation() {
    let ctx = PackageSources::new()
        .schema(
            ["chat"],
            "struct Message {\n    body: str\n}\n",
        )
        .compile()
        .expect("should compile");

    assert!(ctx.config_frozen.is_some(), "config_frozen must be set");
    assert_eq!(ctx.schema_contexts.len(), 1);

    let schema = ctx.schema_contexts[0].borrow();
    assert_eq!(schema.namespace, vec!["chat".to_string()]);
    let frozen = schema.frozen_schema.borrow();
    assert!(frozen.is_some(), "schema should have been interpreted");
}

#[test]
fn honours_an_explicit_congregation() {
    let config = "congregation my_app\nspecification_version = 1\n\n\
                  code_generation = {\n    languages = {\n        rust#1.70.0 = {}\n    }\n}\n";

    let ctx = PackageSources::new()
        .config(config)
        .schema(["ping"], "struct Ping {\n    seq: u32\n}\n")
        .compile()
        .expect("should compile");

    assert_eq!(ctx.config.name.value, "my_app");
}

#[test]
fn multiple_schemas_are_all_interpreted() {
    let ctx = PackageSources::new()
        .schema(["types"], "struct User {\n    id: u64\n}\n")
        .schema(["ping"], "struct Ping {\n    seq: u32\n}\n")
        .schema(["chat"], "struct Message {\n    body: str\n}\n")
        .compile()
        .expect("should compile");

    assert_eq!(ctx.schema_contexts.len(), 3);
    for sc in &ctx.schema_contexts {
        assert!(sc.borrow().frozen_schema.borrow().is_some());
    }
    // The interpretation pass is the same one `compile_package` runs on disk —
    // cross-schema `import` resolution (where `core` supports it) is unaffected
    // by the source being in memory.
}

#[test]
fn nested_namespace_segments_are_kept() {
    let ctx = PackageSources::new()
        .schema(["chat", "admin"], "struct Ban {\n    who: str\n}\n")
        .compile()
        .expect("should compile");

    assert_eq!(
        ctx.schema_contexts[0].borrow().namespace,
        vec!["chat".to_string(), "admin".to_string()]
    );
}

#[test]
fn a_parse_error_is_returned_not_panicked() {
    let err = PackageSources::new()
        .schema(["bad"], "struct {{{ not valid")
        .compile()
        .expect_err("should be an error");

    assert!(err.to_string().contains("bad"), "error names the schema: {err}");
}
