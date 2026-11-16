// What a type written in a schema names: the Rust model (core#290). A name in
// scope is a declaration of the schema, what a `use` brings in (its alias, else
// its last segment) or a root; a whole-namespace `use` brings in only its name;
// a qualified reference starts with a name in scope or a root, and freezes as
// its path from the package's root. The specification's `references` cases hold
// the outcomes (`package/compiler_cases.rs`); these hold what they don't say:
// the diagnostics' fixes, every error at once, and what freezes.

use comline_core::diagnostics::Diagnostic;
use comline_core::package::build::PackageSources;
use comline_core::package::config::idl::grammar as config_grammar;
use comline_core::package::config::ir::compiler::interpret::interpret_context;
use comline_core::package::config::ir::context::ProjectContext;
use comline_core::schema::idl::grammar;
use comline_core::schema::ir::compiler::references::{
    check_references, DEFINED_TWICE, NOT_IN_SCOPE, QUALIFIER_NOT_IN_SCOPE,
};
use comline_core::schema::ir::context::SchemaContext;
use comline_core::schema::ir::frozen::types::TypeRef;
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

/// What the reference check says of the schema `namespace`.
fn errors(project: &ProjectContext, namespace: &[&str]) -> Vec<Diagnostic> {
    let namespace: Vec<String> = namespace.iter().map(|s| s.to_string()).collect();
    let declarations = project
        .schema_contexts
        .iter()
        .find(|schema| schema.borrow().namespace == namespace)
        .expect("schema should exist")
        .borrow()
        .declarations
        .clone();
    check_references(&declarations, &namespace, project)
        .err()
        .unwrap_or_default()
}

const TYPES: &str = "struct User {\n    id: u64\n}\n\nstruct Order {\n    id: u64\n}\n";
const INVOICE: &str = "struct Invoice {\n    data: char[max 64]\n}\n";

/// A project of `types` and what `api` is, and the check's errors for `api`.
fn api_errors(api: &str) -> Vec<Diagnostic> {
    let mut project = build_project();
    add_schema(&mut project, &["types"], TYPES);
    add_schema(&mut project, &["api"], api);
    errors(&project, &["api"])
}

/// `api` as `api` is after the one fix of the one error it has.
fn fixed(api: &str) -> String {
    let errors = api_errors(api);
    assert_eq!(errors.len(), 1, "{api}: {errors:?}");
    errors[0]
        .fix
        .as_ref()
        .unwrap_or_else(|| panic!("{api}: no fix"))
        .apply(api)
}

fn compile(schemas: &[(&[&str], &str)]) -> Result<ProjectContext, String> {
    let mut sources = PackageSources::new();
    for (namespace, source) in schemas {
        sources = sources.schema(namespace.iter().copied(), source.to_string());
    }
    sources.compile().map_err(|e| e.to_string())
}

/// The type of the first field of the struct `A` in the schema `namespace`.
fn field_of_a(project: &ProjectContext, namespace: &str) -> TypeRef {
    let schema = project
        .schema_contexts
        .iter()
        .find(|s| s.borrow().namespace_joined() == namespace)
        .expect("schema should exist");
    let frozen = schema
        .borrow()
        .frozen_schema
        .borrow()
        .clone()
        .expect("frozen");
    frozen
        .iter()
        .find_map(|unit| match unit {
            FrozenUnit::Struct { name, fields, .. } if name == "A" => match &fields[0] {
                FrozenUnit::Field { ty, .. } => Some(ty.clone()),
                _ => None,
            },
            _ => None,
        })
        .expect("struct A")
}

// ---- What a `use` brings into scope.

#[test]
fn a_whole_namespace_use_brings_in_only_its_name() {
    let api = "use package::types\n\nstruct A {\n    user: User\n}\n";
    let errors = api_errors(api);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code, NOT_IN_SCOPE);
    assert_eq!(errors[0].code.id, "reference::not-in-scope");
    assert_eq!(errors[0].message, "'User' is not in scope");
    let help = errors[0].help.as_deref().unwrap();
    assert!(help.contains("add `use package::types::User`"), "{help}");
    assert!(help.contains("write `types::User`"), "{help}");

    // The fix adds the `use` that names it, and then it builds.
    let fix = errors[0].fix.as_ref().unwrap();
    assert_eq!(fix.title, "Add `use package::types::User`");
    let with_use = fix.apply(api);
    assert_eq!(
        with_use,
        "use package::types\nuse package::types::User\n\nstruct A {\n    user: User\n}\n"
    );
    compile(&[(&["types"], TYPES), (&["api"], &with_use)]).expect("what the fix writes builds");
    // So does writing it qualified.
    compile(&[
        (&["types"], TYPES),
        (
            &["api"],
            "use package::types\n\nstruct A {\n    user: types::User\n}\n",
        ),
    ])
    .expect("a qualified name builds");
}

#[test]
fn a_name_nothing_declares_has_no_fix_that_adds_a_use() {
    for api in [
        "use package::types\n\nstruct A {\n    user: Nowhere\n}\n",
        "struct A {\n    user: nope::types::User\n}\n",
    ] {
        let errors = api_errors(api);
        assert_eq!(errors.len(), 1, "{api}: {errors:?}");
        assert!(errors[0].fix.is_none(), "{api}: {:?}", errors[0].fix);
    }
    let errors = api_errors("struct A {\n    user: nope::types::User\n}\n");
    assert_eq!(errors[0].code, QUALIFIER_NOT_IN_SCOPE);
    assert_eq!(errors[0].code.id, "reference::qualifier-not-in-scope");
}

#[test]
fn an_alias_replaces_the_name() {
    let api = "use package::types as t\n\nstruct A {\n    user: types::User\n}\n";
    let errors = api_errors(api);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code, QUALIFIER_NOT_IN_SCOPE);
    assert_eq!(
        fixed(api),
        "use package::types as t\n\nstruct A {\n    user: t::User\n}\n",
        "the alias is what comes into scope"
    );
}

#[test]
fn an_item_or_a_list_does_not_bring_in_its_namespace() {
    for api in [
        "use package::types::User\n\nstruct A {\n    user: types::User\n}\n",
        "use package::types::{User, Order}\n\nstruct A {\n    user: types::User\n}\n",
        "use package::types::*\n\nstruct A {\n    user: types::User\n}\n",
    ] {
        let errors = api_errors(api);
        assert_eq!(errors.len(), 1, "{api}: {errors:?}");
        assert_eq!(errors[0].code, QUALIFIER_NOT_IN_SCOPE, "{api}");
        // `User` is in scope already, so that is what to write.
        assert_eq!(
            fixed(api),
            api.replace("types::User\n}", "User\n}"),
            "{api}"
        );
        assert!(
            errors[0].help.as_deref().unwrap().contains("write `User`"),
            "{api}"
        );
    }
}

#[test]
fn a_qualifier_is_a_root_or_a_name_in_scope() {
    // No `use`: `types` is a schema, not a name in scope. The fixes write
    // the path from `package`.
    let api = "struct A {\n    user: types::User\n}\n";
    let errors = api_errors(api);
    assert_eq!(errors[0].code, QUALIFIER_NOT_IN_SCOPE);
    assert_eq!(
        fixed(api),
        "struct A {\n    user: package::types::User\n}\n"
    );
    assert_eq!(
        fixed("struct A {\n    user: User\n}\n"),
        "use package::types::User\n\nstruct A {\n    user: User\n}\n",
        "with no `use` at all, the fix is put before the first declaration"
    );

    // A segment skipped: `api` is under `billing`, which is in scope.
    let mut project = build_project();
    add_schema(&mut project, &["billing", "api"], INVOICE);
    let skipping = "use package::billing\n\nstruct A {\n    i: api::Invoice\n}\n";
    add_schema(&mut project, &["app"], skipping);
    let errors = errors(&project, &["app"]);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code, QUALIFIER_NOT_IN_SCOPE);
    assert_eq!(
        errors[0].fix.as_ref().unwrap().apply(skipping),
        "use package::billing\n\nstruct A {\n    i: billing::api::Invoice\n}\n"
    );
}

// ---- A name defined twice.

#[test]
fn a_declaration_and_a_use_of_one_name_is_defined_twice() {
    let mut project = build_project();
    add_schema(&mut project, &["billing", "api"], INVOICE);
    let api = "use package::billing::api::Invoice\n\nstruct Invoice {\n    data: char[max 64]\n}\n";
    add_schema(&mut project, &["api"], api);

    let errors = errors(&project, &["api"]);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code, DEFINED_TWICE);
    assert_eq!(errors[0].code.id, "reference::defined-twice");
    assert_eq!(
        errors[0].message,
        "'Invoice' is defined twice in this schema"
    );
    assert!(
        errors[0].secondary.is_some(),
        "the other place is shown too"
    );
    let aliased = errors[0].fix.as_ref().unwrap().apply(api);
    assert_eq!(
        aliased,
        "use package::billing::api::Invoice as BillingInvoice\n\nstruct Invoice {\n    data: char[max 64]\n}\n"
    );
    compile(&[(&["billing", "api"], INVOICE), (&["api"], &aliased)])
        .expect("an alias defines each name once");
}

#[test]
fn two_uses_of_one_name_are_defined_twice_and_the_same_use_twice_is_not() {
    let mut project = build_project();
    add_schema(&mut project, &["a"], TYPES);
    add_schema(&mut project, &["b"], TYPES);
    let api = "use package::a::User\nuse package::b::User\n\nstruct A {\n    id: u64\n}\n";
    add_schema(&mut project, &["api"], api);
    let errors = errors(&project, &["api"]);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code, DEFINED_TWICE);
    // The later `use` takes the alias.
    assert_eq!(
        errors[0].fix.as_ref().unwrap().apply(api),
        "use package::a::User\nuse package::b::User as BUser\n\nstruct A {\n    id: u64\n}\n"
    );

    // The same thing brought in twice is redundant, not an error; and a name
    // from a `*` gives way to a declaration or a `use` of it.
    for api in [
        "use package::a::User\nuse package::a::User\n\nstruct A {\n    u: User\n}\n",
        "use package::a::*\n\nstruct User {\n    id: u8\n}\n\nstruct A {\n    u: User\n}\n",
        "use package::a::*\nuse package::b::User\n\nstruct A {\n    u: User\n}\n",
    ] {
        let mut project = build_project();
        add_schema(&mut project, &["a"], TYPES);
        add_schema(&mut project, &["b"], TYPES);
        add_schema(&mut project, &["api"], api);
        assert!(
            errors_of(&project).is_empty(),
            "{api}: {:?}",
            errors_of(&project)
        );
    }

    fn errors_of(project: &ProjectContext) -> Vec<Diagnostic> {
        errors(project, &["api"])
    }
}

// ---- Every error is reported; an import that fails brings its name in.

#[test]
fn every_error_is_reported_not_the_first() {
    let mut project = build_project();
    add_schema(&mut project, &["types"], TYPES);
    let api = "use package::types\n\nstruct User {\n    id: u8\n}\n\nstruct A {\n    first: Order\n    second: nope::User\n    third: types::Order\n}\n";
    add_schema(&mut project, &["api"], api);
    let errors = errors(&project, &["api"]);
    let codes: Vec<&str> = errors.iter().map(|e| e.code.id).collect();
    assert_eq!(
        codes,
        [
            "reference::not-in-scope",
            "reference::qualifier-not-in-scope"
        ],
        "{errors:?}"
    );
    // The errors point at the names, in the order written.
    let at = |error: &Diagnostic| {
        let (start, end) = error.span.unwrap();
        &api[start..end]
    };
    assert_eq!((at(&errors[0]), at(&errors[1])), ("Order", "nope::User"));
}

#[test]
fn an_import_and_a_reference_are_both_reported_by_the_build() {
    let error = compile(&[
        (&["models"], TYPES),
        (&["api"], "use models\n\nstruct A {\n    u: User\n}\n"),
    ])
    .err()
    .expect("both are errors");
    assert!(error.contains("import::bare-own-schema-path"), "{error}");
    assert!(error.contains("reference::not-in-scope"), "{error}");
}

#[test]
fn a_use_that_fails_to_resolve_still_brings_its_name_in() {
    let mut project = build_project();
    add_schema(&mut project, &["models"], TYPES);
    add_schema(
        &mut project,
        &["api"],
        "use modles::User\n\nstruct A {\n    u: User\n}\n",
    );
    assert!(errors(&project, &["api"]).is_empty());
    let error = compile(&[
        (&["models"], TYPES),
        (&["api"], "use modles::User\n\nstruct A {\n    u: User\n}\n"),
    ])
    .err()
    .expect("the import is the one error");
    assert!(
        error.contains("import::unresolved") && !error.contains("reference::"),
        "{error}"
    );
}

// ---- What a qualified reference freezes as.

#[test]
fn a_qualified_reference_freezes_as_its_resolved_path() {
    let spellings = [
        "use package::billing::api\n\nstruct A {\n    i: api::Invoice\n}\n",
        "use package::billing\n\nstruct A {\n    i: billing::api::Invoice\n}\n",
        "use package::billing as b\n\nstruct A {\n    i: b::api::Invoice\n}\n",
        "use package::billing::api as b\n\nstruct A {\n    i: b::Invoice\n}\n",
        "struct A {\n    i: package::billing::api::Invoice\n}\n",
    ];
    for api in spellings {
        let project = compile(&[(&["billing", "api"], INVOICE), (&["app"], api)])
            .unwrap_or_else(|e| panic!("{api}: {e}"));
        assert_eq!(
            field_of_a(&project, "app"),
            TypeRef::Named("billing::api::Invoice".into()),
            "{api}"
        );
    }

    // A bare name, and a qualified one that is already a path, are as written.
    let project = compile(&[
        (&["billing", "api"], INVOICE),
        (
            &["app"],
            "use package::billing::api::Invoice\n\nstruct A {\n    i: Invoice\n}\n",
        ),
    ])
    .unwrap();
    assert_eq!(
        field_of_a(&project, "app"),
        TypeRef::Named("Invoice".into())
    );
    let project = compile(&[
        (&["types"], TYPES),
        (
            &["app"],
            "use package::types\n\nstruct A {\n    u: types::User\n}\n",
        ),
    ])
    .unwrap();
    assert_eq!(
        field_of_a(&project, "app"),
        TypeRef::Named("types::User".into())
    );
}

#[test]
fn what_is_written_the_same_gives_the_same_frozen_schema_whichever_the_spelling() {
    // The spans differ with the text, so only what the names freeze as is held.
    let frozen = |api: &str| {
        let project = compile(&[(&["billing", "api"], INVOICE), (&["app"], api)]).unwrap();
        field_of_a(&project, "app").to_string()
    };
    assert_eq!(
        frozen("use package::billing::api\n\nstruct A {\n    i: api::Invoice[]\n}\n"),
        frozen("struct A {\n    i: package::billing::api::Invoice[]\n}\n"),
    );
}

#[test]
fn a_full_path_from_a_root_needs_no_use() {
    let shapes = [
        (&["shapes", "types"][..], TYPES),
        (
            &["shapes", "api"][..],
            "struct A {\n    u: package::shapes::types::User\n    p: parent::types::Order\n}\n",
        ),
    ];
    let project = compile(&shapes).expect("package:: and parent::");
    assert_eq!(
        field_of_a(&project, "shapes::api"),
        TypeRef::Named("shapes::types::User".into())
    );

    let nested = [
        (&["api"][..], "struct A {\n    u: self::nested::User\n}\n"),
        (&["api", "nested"][..], TYPES),
    ];
    let project = compile(&nested).expect("self::");
    assert_eq!(
        field_of_a(&project, "api"),
        TypeRef::Named("api::nested::User".into())
    );

    let std = [(&["api"][..], "struct A {\n    r: std::http::Request\n}\n")];
    let project = compile(&std).expect("std:: merges the schema it names, with no use");
    assert_eq!(
        field_of_a(&project, "api"),
        TypeRef::Named("std::http::Request".into())
    );
}

#[test]
fn an_alias_target_and_a_function_read_qualified_names_too() {
    let api = "use package::billing\n\ntype Id = billing::api::Invoice\n\nprotocol P {\n    function get(i: billing::api::Invoice) -> Id;\n}\n";
    let project = compile(&[(&["billing", "api"], INVOICE), (&["app"], api)]).expect("builds");
    let schema = project
        .schema_contexts
        .iter()
        .find(|s| s.borrow().namespace_joined() == "app")
        .unwrap();
    let frozen = schema.borrow().frozen_schema.borrow().clone().unwrap();
    let alias = frozen.iter().find_map(|u| match u {
        FrozenUnit::TypeAlias { target, .. } => Some(target.clone()),
        _ => None,
    });
    assert_eq!(alias, Some(TypeRef::Named("billing::api::Invoice".into())));
}

// ---- A package written before the rule freezes as it did.

#[test]
fn a_whole_namespace_use_and_a_qualified_name_freeze_as_they_always_did() {
    let project = compile(&[
        (&["types"], TYPES),
        (
            &["api"],
            "use package::types\n\nstruct A {\n    user: types::User\n}\n",
        ),
    ])
    .unwrap();
    let schema = project
        .schema_contexts
        .iter()
        .find(|s| s.borrow().namespace_joined() == "api")
        .unwrap();
    let frozen = schema.borrow().frozen_schema.borrow().clone().unwrap();
    let imports: Vec<&str> = frozen
        .iter()
        .filter_map(|u| match u {
            FrozenUnit::Import(path, _, _) => Some(path.as_str()),
            _ => None,
        })
        .collect();
    // The import is still one per name the namespace has: what is frozen is
    // what it was, whatever the file may now write.
    assert_eq!(imports, ["types", "types::User", "types::Order"]);
}
