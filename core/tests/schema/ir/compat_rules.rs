// The compatibility rules table (core#119), one row per case: an old and
// a new schema, the bump the diff computes and the change kind it reports.
// Format-dependent rows run under each branch of the profile fact they
// read.

use comline_core::schema::ir::compiler::interpreter::incremental::IncrementalInterpreter;
use comline_core::schema::ir::compiler::Compile;
use comline_core::schema::ir::diff::{
    analyze_package_changes, analyze_package_changes_with, analyze_schema_changes, SchemaChanges,
    WireFacts,
};
use comline_core::schema::ir::frozen::unit::FrozenUnit;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bump {
    None,
    Patch,
    Minor,
    Major,
}

fn bump(changes: &SchemaChanges) -> Bump {
    if !changes.breaking_changes.is_empty() {
        Bump::Major
    } else if !changes.new_features.is_empty() {
        Bump::Minor
    } else if !changes.modifications.is_empty() {
        Bump::Patch
    } else {
        Bump::None
    }
}

/// Every change's `kind`, as serialized.
fn kinds(changes: &SchemaChanges) -> Vec<String> {
    let kind = |v: serde_json::Value| v["kind"].as_str().unwrap().to_string();
    let mut all: Vec<String> = changes.breaking_changes.iter().map(|c| kind(serde_json::to_value(c).unwrap())).collect();
    all.extend(changes.new_features.iter().map(|c| kind(serde_json::to_value(c).unwrap())));
    all.extend(changes.modifications.iter().map(|c| kind(serde_json::to_value(c).unwrap())));
    all
}

fn units(code: &str) -> Vec<FrozenUnit> {
    let mut units = vec![FrozenUnit::Namespace("pkg::api".to_string())];
    units.extend(IncrementalInterpreter::from_source(code));
    units
}

/// The diff under core's defaults (`packed@1`, `datagram@2`), or the
/// framing a protocol names.
fn default(old: &str, new: &str) -> SchemaChanges {
    analyze_schema_changes(&units(old), &units(new))
}

/// The diff under the given facts.
fn under(facts: &WireFacts, old: &str, new: &str) -> SchemaChanges {
    analyze_package_changes_with(&[units(old)], &[units(new)], facts)
}

/// What `json@1` with `jsonrpc@1` states, from their profiles.
fn json() -> WireFacts {
    let jsonrpc = units("@framing = \"jsonrpc\"\nprotocol P {\n    function f();\n}\n");
    WireFacts::of((std::slice::from_ref(&jsonrpc), &[]), (std::slice::from_ref(&jsonrpc), &[]))
}

fn check(rows: &[(&str, SchemaChanges, Bump, &str)]) {
    for (row, changes, expected, kind) in rows {
        assert_eq!(bump(changes), *expected, "{row}: {changes:?}");
        if !kind.is_empty() {
            assert!(kinds(changes).iter().any(|k| k == kind), "{row}: expected `{kind}` in {:?}", kinds(changes));
        } else {
            assert!(changes.is_empty(), "{row}: {changes:?}");
        }
    }
}

const BOUNDS: &str = "validator Bounds for string {\n    max: u32 = 10\n    validate {\n        \
                      assert(value.length <= params.max, f\"{value.name} is too long\")\n    }\n}\n";

#[test]
fn the_profiles_give_the_facts() {
    let default = WireFacts::of((&[units("")], &[]), (&[units("")], &[]));
    assert_eq!(default, WireFacts::default(), "packed@1 is the strictest");
    let json = json();
    assert!(json.fields_by_name && json.optional_fields_tolerated && json.variants_by_name);
    assert!(!json.errors_by_name, "jsonrpc@1 sends errors by ordinal");
    assert_eq!(json.calls_stable.get("pkg::api::P"), Some(&true));
    // A framing with no profile core can read is the strictest.
    let unknown = units("@framing = \"com.acme.frames@1\"\nprotocol P {\n    function f();\n}\n");
    let facts = WireFacts::of((std::slice::from_ref(&unknown), &[]), (std::slice::from_ref(&unknown), &[]));
    assert_eq!(facts.calls_stable.get("pkg::api::P"), Some(&false));
}

#[test]
fn fields() {
    let s = |body: &str| format!("struct S {{\n{body}}}\n");
    let json = json();
    check(&[
        ("add optional, by_name", under(&json, &s("    a: u32\n"), &s("    a: u32\n    optional b: u32\n")), Bump::Minor, "added_field"),
        ("add optional, by_position", default(&s("    a: u32\n"), &s("    a: u32\n    optional b: u32\n")), Bump::Major, "added_optional_field"),
        ("add required", under(&json, &s("    a: u32\n"), &s("    a: u32\n    b: u32\n")), Bump::Major, "added_required_field"),
        ("remove", under(&json, &s("    a: u32\n    optional b: u32\n"), &s("    a: u32\n")), Bump::Major, "removed_field"),
        ("made optional", under(&json, &s("    a: u32\n"), &s("    optional a: u32\n")), Bump::Major, "changed_field_optionality"),
        ("made required", under(&json, &s("    optional a: u32\n"), &s("    a: u32\n")), Bump::Major, "changed_field_optionality"),
        ("reorder, by_name", under(&json, &s("    a: u32\n    b: u32\n"), &s("    b: u32\n    a: u32\n")), Bump::None, ""),
        ("reorder, by_position", default(&s("    a: u32\n    b: u32\n"), &s("    b: u32\n    a: u32\n")), Bump::Major, "reordered_fields"),
        ("change type", default(&s("    a: u32\n"), &s("    a: u64\n")), Bump::Major, "changed_field_type"),
        ("default", default(&s("    a: u32 = 1\n"), &s("    a: u32 = 2\n")), Bump::Patch, "changed_field_default"),
        ("docstring", default(&s("    a: u32\n"), &s("    /// The a.\n    a: u32\n")), Bump::Patch, "docstring_changed"),
    ]);
}

#[test]
fn aliases() {
    let handle = "type Handle = string\n";
    let other = "type Name = string\n";
    let field = |ty: &str| format!("{handle}{other}struct S {{\n    a: {ty}\n}}\n");
    check(&[
        // core#110: an alias swap, either way, or alias for alias.
        ("to alias", default(&field("string"), &field("Handle")), Bump::Patch, "type_respelled"),
        ("from alias", default(&field("Handle"), &field("string")), Bump::Patch, "type_respelled"),
        ("alias for alias", default(&field("Handle"), &field("Name")), Bump::Patch, "type_respelled"),
        ("add", default("", handle), Bump::Minor, "added_type_alias"),
        ("remove", default(handle, ""), Bump::Major, "removed_type_alias"),
        (
            "retarget, at its use",
            default(&field("Handle"), &field("Handle").replace("Handle = string", "Handle = u64")),
            Bump::Major,
            "changed_field_type",
        ),
    ]);
    let retargeted = default(&field("Handle"), &field("Handle").replace("Handle = string", "Handle = u64"));
    assert!(kinds(&retargeted).contains(&"changed_type_alias_target".to_string()));
    let args = |ty: &str| format!("{handle}protocol P {{\n    function f(a: {ty}) -> {ty};\n}}\n");
    check(&[("argument respelled", default(&args("string"), &args("Handle")), Bump::Patch, "type_respelled")]);
}

#[test]
fn enums() {
    let e = |body: &str| format!("enum E {{\n{body}}}\n");
    let json = json();
    check(&[
        ("add variant, by_name", under(&json, &e("    A\n"), &e("    A\n    B\n")), Bump::Minor, "added_enum_variant"),
        ("append variant, by_index", default(&e("    A\n"), &e("    A\n    B\n")), Bump::Minor, "added_enum_variant"),
        ("insert variant, by_index", default(&e("    A\n"), &e("    B\n    A\n")), Bump::Major, "reordered_enum_variants"),
        ("remove variant", default(&e("    A\n    B\n"), &e("    A\n")), Bump::Major, "removed_enum_variant"),
        ("rename variant", default(&e("    A\n    B\n"), &e("    A\n    C\n")), Bump::Major, "renamed_enum_variant"),
        ("reorder, by_name", under(&json, &e("    A\n    B\n"), &e("    B\n    A\n")), Bump::None, ""),
        ("reorder, by_index", default(&e("    A\n    B\n"), &e("    B\n    A\n")), Bump::Major, "reordered_enum_variants"),
    ]);
}

#[test]
fn functions() {
    let p = |body: &str| format!("protocol P {{\n{body}}}\n");
    let rpc = |body: &str| format!("@framing = \"jsonrpc\"\n{}", p(body));
    let errors = "error A {\n    message = \"a\"\n}\nerror B {\n    message = \"b\"\n}\n";
    check(&[
        ("add", default(&p("    function f();\n"), &p("    function f();\n    function g();\n")), Bump::Minor, "added_function"),
        ("remove", default(&p("    function f();\n    function g();\n"), &p("    function f();\n")), Bump::Major, "removed_function"),
        ("reorder, by_index", default(&p("    function f();\n    function g();\n"), &p("    function g();\n    function f();\n")), Bump::Major, "reordered_functions"),
        ("insert before, by_index", default(&p("    function f();\n"), &p("    function g();\n    function f();\n")), Bump::Major, "reordered_functions"),
        ("reorder, by_name", default(&rpc("    function f();\n    function g();\n"), &rpc("    function g();\n    function f();\n")), Bump::None, ""),
        ("insert before, by_name", default(&rpc("    function f();\n"), &rpc("    function g();\n    function f();\n")), Bump::Minor, "added_function"),
        ("rename argument", default(&p("    function f(a: u32);\n"), &p("    function f(b: u32);\n")), Bump::Major, "changed_function_signature"),
        ("reorder arguments", default(&p("    function f(a: u32, b: u32);\n"), &p("    function f(b: u32, a: u32);\n")), Bump::Major, "changed_function_signature"),
        ("return type", default(&p("    function f() -> u32;\n"), &p("    function f() -> u64;\n")), Bump::Major, "changed_function_signature"),
        ("one-way to request", default(&p("    function f();\n"), &p("    function f() -> ();\n")), Bump::Major, "changed_function_signature"),
        ("add error", default(&format!("{errors}{}", p("    function f() ! A;\n")), &format!("{errors}{}", p("    function f() ! A, B;\n"))), Bump::Minor, "added_thrown_error"),
        ("remove error", default(&format!("{errors}{}", p("    function f() ! A, B;\n")), &format!("{errors}{}", p("    function f() ! A;\n"))), Bump::Major, "removed_thrown_error"),
        ("docstring", default(&p("    function f();\n"), &p("    /// Does f.\n    function f();\n")), Bump::Patch, "docstring_changed"),
        ("timeout", default(&p("    function f();\n"), &p("    @timeout_ms = 10\n    function f();\n")), Bump::Patch, "changed_annotation"),
    ]);
}

#[test]
fn errors() {
    let a = "error A {\n    message = \"a\"\n}\n";
    let shifted = format!("error Z {{\n    message = \"z\"\n}}\n{a}");
    let by_name = WireFacts { errors_by_name: true, ..WireFacts::default() };
    check(&[
        ("ordinal shifts, by_ordinal", default(a, &shifted), Bump::Major, "changed_error_ordinal"),
        ("append", default(a, &format!("{a}error B {{\n    message = \"b\"\n}}\n")), Bump::Minor, "added_error"),
        ("message", default(a, "error A {\n    message = \"b\"\n}\n"), Bump::Patch, "changed_error_message"),
        ("field added", default(a, "error A {\n    message = \"a\"\n    id: u64\n}\n"), Bump::Major, "added_required_field"),
    ]);
    // Where errors travel by name, the ordinal doesn't.
    let changes = under(&by_name, a, &shifted);
    assert!(!kinds(&changes).contains(&"changed_error_ordinal".to_string()), "{changes:?}");
}

#[test]
fn constants() {
    let validated = |v: u32| format!("const MAX: u32 = {v}\n{BOUNDS}struct S {{\n    @validators = [Bounds(max = MAX)]\n    a: string\n}}\n");
    let defaulted = |v: u32| format!("const MAX: u32 = {v}\nstruct S {{\n    a: u32 = MAX\n}}\n");
    check(&[
        ("add", default("", "const MAX: u32 = 1\n"), Bump::Minor, "added_constant"),
        ("remove", default("const MAX: u32 = 1\n", ""), Bump::Major, "removed_constant"),
        ("type", default("const MAX: u32 = 1\n", "const MAX: u64 = 1\n"), Bump::Major, "changed_constant_type"),
        ("value, unused", default("const MAX: u32 = 1\n", "const MAX: u32 = 2\n"), Bump::Minor, "changed_constant_value"),
        ("value, a default", default(&defaulted(1), &defaulted(2)), Bump::Patch, "changed_constant_value"),
        ("value, a validator argument", default(&validated(1), &validated(2)), Bump::Major, "validation_tightened"),
    ]);
}

#[test]
fn validators() {
    let none = format!("{BOUNDS}struct S {{\n    a: string\n}}\n");
    let own = format!("{BOUNDS}struct S {{\n    @validators = [Bounds()]\n    a: string\n}}\n");
    let alias = |ty: &str| format!("{BOUNDS}@validators = [Bounds()]\ntype Short = string\nstruct S {{\n    a: {ty}\n}}\n");
    check(&[
        ("applied", default(&none, &own), Bump::Major, "validation_tightened"),
        ("removed", default(&own, &none), Bump::Minor, "validation_loosened"),
        // The effective set includes the alias chain's validators.
        ("through an alias", default(&alias("string"), &alias("Short")), Bump::Major, "validation_tightened"),
        ("alias dropped", default(&alias("Short"), &alias("string")), Bump::Minor, "validation_loosened"),
        ("add declaration", default("", BOUNDS), Bump::Minor, "added_validator"),
        ("remove declaration", default(BOUNDS, ""), Bump::Major, "removed_validator"),
        ("change declaration", default(BOUNDS, &BOUNDS.replace("= 10", "= 5")), Bump::Major, "changed_validator"),
    ]);
}

/// core#156: a call is compared with its arguments resolved against the
/// validator's parameter defaults.
#[test]
fn validator_arguments_resolve_against_defaults() {
    let uses = |bounds: &str, call: &str| {
        format!("{bounds}struct S {{\n    @validators = [{call}]\n    a: string\n}}\n")
    };
    let changes = default(&uses(BOUNDS, "Bounds()"), &uses(&BOUNDS.replace("= 10", "= 5"), "Bounds()"));
    let tightened = changes.breaking_changes.iter().any(|c| {
        serde_json::to_value(c).unwrap() == serde_json::json!({"kind": "validation_tightened", "target": "S.a"})
    });
    assert!(tightened, "a narrowed default narrows the call relying on it: {changes:?}");

    // Writing the default out, or leaving it out, is no change to the call.
    let changes = default(&uses(BOUNDS, "Bounds()"), &uses(BOUNDS, "Bounds(max = 10)"));
    assert!(changes.is_empty(), "{changes:?}");
}

#[test]
fn annotations_and_docstrings() {
    let p = |annotation: &str| format!("{annotation}protocol P {{\n    function f();\n}}\n");
    let s = |annotation: &str| format!("{annotation}struct S {{\n    a: u32\n}}\n");
    check(&[
        ("framing", default(&p(""), &p("@framing = \"jsonrpc\"\n")), Bump::Major, "changed_wire_annotation"),
        ("handshake", default(&p(""), &p("@handshake = \"none\"\n")), Bump::Major, "changed_wire_annotation"),
        ("deprecated added", default(&s(""), &s("@deprecated = \"use T\"\n")), Bump::Minor, "deprecated"),
        ("deprecated removed", default(&s("@deprecated = \"use T\"\n"), &s("")), Bump::Patch, "changed_annotation"),
        ("deprecated reworded", default(&s("@deprecated = \"use T\"\n"), &s("@deprecated = \"use U\"\n")), Bump::Patch, "changed_annotation"),
        ("unknown annotation", default(&s(""), &s("@color = \"red\"\n")), Bump::Patch, "changed_annotation"),
        ("docstring", default(&s(""), &s("/// An S.\n")), Bump::Patch, "docstring_changed"),
    ]);
}

#[test]
fn renames() {
    let protocol = |name: &str| format!("protocol {name} {{\n    function f();\n}}\n");
    check(&[("protocol", default(&protocol("P"), &protocol("Q")), Bump::Major, "renamed_protocol")]);
    let renamed = default(&protocol("P"), &protocol("Q"));
    assert_eq!(kinds(&renamed), ["renamed_protocol"], "compared as a pair, not a removal plus an addition");

    let schema = |ns: &str| {
        let mut units = vec![FrozenUnit::Namespace(ns.to_string())];
        units.extend(IncrementalInterpreter::from_source("struct S {\n    a: u32\n}\n"));
        units
    };
    let changes = analyze_package_changes(&[schema("pkg::a")], &[schema("pkg::b")]);
    assert_eq!(kinds(&changes), ["renamed_namespace"]);
    assert_eq!(bump(&changes), Bump::Major);
}
