// `use std::…`: the embedded standard library, through the real compile path
// (`PackageSources`, which merges the std schemas a package imports).

use comline_core::package::build::PackageSources;
use comline_core::package::config::ir::context::ProjectContext;
use comline_core::package::stdlib;

fn namespaces(context: &ProjectContext) -> Vec<String> {
    context.schema_contexts.iter().map(|schema| schema.borrow().namespace_joined()).collect()
}

fn compile(schemas: &[(&[&str], &str)]) -> eyre::Result<ProjectContext> {
    let mut sources = PackageSources::new();
    for (namespace, source) in schemas {
        sources = sources.schema(namespace.iter().copied(), *source);
    }
    sources.compile()
}

#[test]
fn a_std_type_is_usable() {
    let context = compile(&[(
        &["api"],
        "use std::http::{Request, Response}\n\nstruct Exchange {\n    request: Request\n    response: Response\n}\n",
    )])
    .expect("std types should resolve");

    assert_eq!(namespaces(&context), ["api", "std::http"], "only the std schema it imports");
}

#[test]
fn a_std_validator_is_usable() {
    compile(&[(
        &["users"],
        "use std::validators::StringBounds\n\nstruct User {\n    @validators = [StringBounds(min_chars = 3, max_chars = 32)]\n    name: str\n}\n",
    )])
    .expect("a std validator should resolve");
}

#[test]
fn a_package_without_std_gets_none_of_it() {
    let context = compile(&[(&["api"], "struct A {\n    id: u64\n}\n")]).unwrap();
    assert_eq!(namespaces(&context), ["api"]);
}

#[test]
fn a_typo_in_a_std_path_is_rejected_with_a_suggestion() {
    let error = compile(&[(&["api"], "use std::htp::Request\n\nstruct A {\n    r: Request\n}\n")])
        .expect_err("an unknown std path should fail")
        .to_string();
    assert!(error.contains("std has no schema matching 'std::htp::Request' - did you mean 'std::http'?"), "{error}");

    let error = compile(&[(&["api"], "use std::http::Reqest\n\nstruct A {\n    id: u64\n}\n")])
        .expect_err("an undeclared std item should fail")
        .to_string();
    assert!(error.contains("schema 'std::http' doesn't declare 'Reqest' - did you mean 'Request'?"), "{error}");
}

#[test]
fn every_std_schema_compiles() {
    let mut sources = PackageSources::new().config(stdlib::manifest());
    for (namespace, source) in stdlib::schemas() {
        sources = sources.schema(namespace[1..].to_vec(), source);
    }
    sources.compile().expect("the std package should compile on its own");
}

#[test]
fn std_and_each_of_its_modules_document_themselves() {
    use comline_core::schema::idl::module_docs::module_docs;

    assert!(module_docs(stdlib::manifest()).is_some(), "std's config.idp needs a `//!` header");
    for (namespace, source) in stdlib::schemas() {
        assert!(
            module_docs(source).is_some(),
            "std::{} needs a `//!` module docstring",
            namespace[1..].join("::")
        );
    }
}
