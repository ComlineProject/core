// The gaps core#118 listed in the schema diff: changes that went unreported
// whatever the compatibility rules end up being.

use comline_core::schema::ir::compiler::interpreter::incremental::IncrementalInterpreter;
use comline_core::schema::ir::compiler::Compile;
use comline_core::schema::ir::diff::{
    analyze_package_changes, analyze_schema_changes, BreakingChange, Modification, NewFeature,
    SchemaChanges,
};
use comline_core::schema::ir::frozen::unit::FrozenUnit;

fn diff(old: &str, new: &str) -> SchemaChanges {
    analyze_schema_changes(
        &IncrementalInterpreter::from_source(old),
        &IncrementalInterpreter::from_source(new),
    )
}

const BOUNDS: &str = "validator Bounds for string {\n    max: u32 = 10\n    validate {\n        \
                      assert(value.length <= params.max, f\"{value.name} is too long\")\n    }\n}\n";

#[test]
fn removing_an_optional_field_is_breaking() {
    let changes = diff(
        "struct S {\n    a: u32\n    optional b: string\n}\n",
        "struct S {\n    a: u32\n}\n",
    );
    assert!(matches!(
        changes.breaking_changes.as_slice(),
        [BreakingChange::RemovedField { field_name, .. }] if field_name == "b"
    ));
}

#[test]
fn an_optional_flip_is_breaking_either_way() {
    let made_optional = diff("struct S {\n    a: u32\n}\n", "struct S {\n    optional a: u32\n}\n");
    assert!(matches!(
        made_optional.breaking_changes.as_slice(),
        [BreakingChange::ChangedFieldOptionality { now_optional: true, .. }]
    ));

    let made_required = diff("struct S {\n    optional a: u32\n}\n", "struct S {\n    a: u32\n}\n");
    assert!(matches!(
        made_required.breaking_changes.as_slice(),
        [BreakingChange::ChangedFieldOptionality { now_optional: false, .. }]
    ));
}

#[test]
fn a_field_default_change_is_a_patch() {
    let changes = diff("struct S {\n    a: u32 = 1\n}\n", "struct S {\n    a: u32 = 2\n}\n");
    assert!(changes.breaking_changes.is_empty() && changes.new_features.is_empty());
    assert!(matches!(
        changes.modifications.as_slice(),
        [Modification::ChangedFieldDefault { field_name, .. }] if field_name == "a"
    ));
}

#[test]
fn renaming_an_argument_changes_the_signature() {
    let changes = diff(
        "protocol P {\n    function f(id: u64);\n}\n",
        "protocol P {\n    function f(key: u64);\n}\n",
    );
    assert!(matches!(
        changes.breaking_changes.as_slice(),
        [BreakingChange::ChangedFunctionSignature { details, .. }]
            if details == "(id: u64) -> void → (key: u64) -> void"
    ));
}

#[test]
fn constants_are_compared() {
    let added = diff("", "const MAX: u32 = 10\n");
    assert!(matches!(added.new_features.as_slice(), [NewFeature::AddedConstant { .. }]));

    let removed = diff("const MAX: u32 = 10\n", "");
    assert!(matches!(removed.breaking_changes.as_slice(), [BreakingChange::RemovedConstant { .. }]));

    let retyped = diff("const MAX: u32 = 10\n", "const MAX: u64 = 10\n");
    assert!(matches!(
        retyped.breaking_changes.as_slice(),
        [BreakingChange::ChangedConstantType { .. }]
    ));

    let revalued = diff("const MAX: u32 = 10\n", "const MAX: u32 = 20\n");
    assert!(matches!(
        revalued.new_features.as_slice(),
        [NewFeature::ChangedConstantValue { old_value: Some(o), new_value: Some(n), .. }]
            if o == "10" && n == "20"
    ));
}

#[test]
fn aliases_are_compared() {
    let added = diff("", "type Id = u64\n");
    assert!(matches!(added.new_features.as_slice(), [NewFeature::AddedTypeAlias { .. }]));

    let removed = diff("type Id = u64\n", "");
    assert!(matches!(removed.breaking_changes.as_slice(), [BreakingChange::RemovedTypeAlias { .. }]));

    let retargeted = diff("type Id = u64\n", "type Id = string\n");
    assert!(matches!(
        retargeted.breaking_changes.as_slice(),
        [BreakingChange::ChangedTypeAliasTarget { .. }]
    ));
}

#[test]
fn validator_declarations_are_compared() {
    let added = diff("", BOUNDS);
    assert!(matches!(added.new_features.as_slice(), [NewFeature::AddedValidator { .. }]));

    let removed = diff(BOUNDS, "");
    assert!(matches!(removed.breaking_changes.as_slice(), [BreakingChange::RemovedValidator { .. }]));

    let changed = diff(BOUNDS, &BOUNDS.replace("max: u32 = 10", "max: u32 = 5"));
    assert!(matches!(changed.breaking_changes.as_slice(), [BreakingChange::ChangedValidator { .. }]));

    // Only formatting moved: not a change.
    let reformatted = diff(BOUNDS, &BOUNDS.replace("validate {", "\n    validate {"));
    assert!(reformatted.is_empty(), "{reformatted:?}");
}

#[test]
fn applied_validators_tighten_or_loosen() {
    let none = format!("{BOUNDS}struct S {{\n    name: string\n}}\n");
    let one = format!("{BOUNDS}struct S {{\n    @validators = [Bounds()]\n    name: string\n}}\n");

    let tightened = diff(&none, &one);
    assert!(matches!(
        tightened.breaking_changes.as_slice(),
        [BreakingChange::ValidationTightened { target }] if target == "S.name"
    ));

    let loosened = diff(&one, &none);
    assert!(loosened.breaking_changes.is_empty());
    assert!(matches!(
        loosened.new_features.as_slice(),
        [NewFeature::ValidationLoosened { target }] if target == "S.name"
    ));
}

#[test]
fn errors_compare_ordinal_fields_and_message() {
    let one = "error A {\n    message = \"a\"\n}\n";
    let shifted = diff(one, &format!("error Z {{\n    message = \"z\"\n}}\n{one}"));
    assert!(
        shifted
            .breaking_changes
            .iter()
            .any(|c| matches!(c, BreakingChange::ChangedErrorOrdinal { name, .. } if name == "A")),
        "{shifted:?}"
    );

    let reworded = diff(one, "error A {\n    message = \"b\"\n}\n");
    assert!(matches!(
        reworded.modifications.as_slice(),
        [Modification::ChangedErrorMessage { .. }]
    ));

    let new_field = diff(one, "error A {\n    message = \"a\"\n    id: u64\n}\n");
    assert!(matches!(
        new_field.breaking_changes.as_slice(),
        [BreakingChange::AddedRequiredField { type_name, .. }] if type_name == "A"
    ));
}

#[test]
fn thrown_errors_are_compared_by_name() {
    let errors = "error A {\n    message = \"a\"\n}\nerror B {\n    message = \"b\"\n}\n";
    let changes = diff(
        &format!("{errors}protocol P {{\n    function f() ! A;\n}}\n"),
        &format!("{errors}protocol P {{\n    function f() ! B;\n}}\n"),
    );
    assert!(matches!(
        changes.breaking_changes.as_slice(),
        [BreakingChange::RemovedThrownError { error, .. }] if error == "A"
    ));
    assert!(matches!(
        changes.new_features.as_slice(),
        [NewFeature::AddedThrownError { error, .. }] if error == "B"
    ));
}

#[test]
fn annotations_are_compared() {
    let framing = diff(
        "protocol P {\n    function f();\n}\n",
        "@framing = \"jsonrpc\"\nprotocol P {\n    function f();\n}\n",
    );
    assert!(matches!(
        framing.breaking_changes.as_slice(),
        [BreakingChange::ChangedWireAnnotation { key, .. }] if key == "framing"
    ));

    let timeout = diff(
        "protocol P {\n    function f();\n}\n",
        "protocol P {\n    @timeout_ms = 1000\n    function f();\n}\n",
    );
    assert!(matches!(
        timeout.modifications.as_slice(),
        [Modification::ChangedAnnotation { target, key, .. }]
            if target == "P.f" && key == "timeout_ms"
    ));

    let deprecated = diff(
        "struct S {\n    a: u32\n}\n",
        "@deprecated = \"use T\"\nstruct S {\n    a: u32\n}\n",
    );
    assert!(matches!(
        deprecated.new_features.as_slice(),
        [NewFeature::Deprecated { target }] if target == "S"
    ));
}

#[test]
fn a_renamed_enum_variant_is_reported_as_a_rename() {
    let changes = diff("enum E {\n    A\n    B\n}\n", "enum E {\n    A\n    C\n}\n");
    assert!(matches!(
        changes.breaking_changes.as_slice(),
        [BreakingChange::RenamedEnumVariant { old_name, new_name, .. }]
            if old_name == "B" && new_name == "C"
    ));
    assert!(changes.new_features.is_empty());
}

fn schema(namespace: &str, code: &str) -> Vec<FrozenUnit> {
    let mut units = vec![FrozenUnit::Namespace(namespace.to_string())];
    units.extend(IncrementalInterpreter::from_source(code));
    units
}

#[test]
fn schema_files_are_paired_by_namespace_not_position() {
    let a = schema("pkg::a", "struct A {\n    x: u32\n}\n");
    let b = schema("pkg::b", "struct B {\n    y: u32\n}\n");

    // Same files, other order: nothing changed.
    let reordered = analyze_package_changes(&[a.clone(), b.clone()], &[b.clone(), a.clone()]);
    assert!(reordered.is_empty(), "{reordered:?}");

    // A new file before the others: one addition, nothing removed.
    let n = schema("pkg::n", "struct N {\n    z: u32\n}\n");
    let inserted = analyze_package_changes(&[a.clone(), b.clone()], &[n, a.clone(), b]);
    assert!(inserted.breaking_changes.is_empty(), "{inserted:?}");
    assert!(matches!(
        inserted.new_features.as_slice(),
        [NewFeature::AddedStruct { name, .. }] if name == "N"
    ));

    // A removed file: its declarations are removals.
    let removed = analyze_package_changes(&[a.clone()], &[]);
    assert!(matches!(
        removed.breaking_changes.as_slice(),
        [BreakingChange::RemovedStruct { name }] if name == "A"
    ));
}
