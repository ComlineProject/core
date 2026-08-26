// Confirms the explicit design choice made this session: spans live
// directly on FrozenUnit fields, so the CAS blob hash is sensitive to
// source position/formatting (reformatting = different hash, different
// content-identity), while the human-facing semantic diff used for
// version-bump decisions stays purely structural and ignores spans.

use comline_core::schema::ir::compiler::interpreter::incremental::IncrementalInterpreter;
use comline_core::schema::ir::compiler::Compile;
use comline_core::schema::ir::diff::analyze_schema_changes;
use comline_core::schema::ir::frozen::cas::blob::frozen_unit_to_blob;

#[test]
fn test_reformatting_changes_blob_hash_but_not_semantic_diff() {
    let compact = "struct User {\n    id: u64\n    name: str\n}\n";
    let reformatted = "struct User {\n\n    id: u64\n    name: str\n\n}\n";

    let compact_units = IncrementalInterpreter::from_source(compact);
    let reformatted_units = IncrementalInterpreter::from_source(reformatted);

    assert_eq!(compact_units.len(), 1);
    assert_eq!(reformatted_units.len(), 1);

    // Same semantic content -> the human-facing diff reports no changes.
    let changes = analyze_schema_changes(&compact_units, &reformatted_units);
    assert!(
        changes.is_empty(),
        "Reformatting alone should not register as a semantic change, got {:?}",
        changes
    );

    // But the CAS blob hash - which includes the new `span` fields - does
    // differ, because the two sources aren't byte-identical.
    let compact_hash = frozen_unit_to_blob(&compact_units[0]).unwrap().hash();
    let reformatted_hash = frozen_unit_to_blob(&reformatted_units[0]).unwrap().hash();

    assert_ne!(
        compact_hash.to_hex(),
        reformatted_hash.to_hex(),
        "Expected reformatting to change the content-addressed blob hash"
    );
}

#[test]
fn test_identical_source_produces_identical_hash() {
    let source = "struct User {\n    id: u64\n}\n";

    let units_a = IncrementalInterpreter::from_source(source);
    let units_b = IncrementalInterpreter::from_source(source);

    let hash_a = frozen_unit_to_blob(&units_a[0]).unwrap().hash();
    let hash_b = frozen_unit_to_blob(&units_b[0]).unwrap().hash();

    assert_eq!(
        hash_a.to_hex(),
        hash_b.to_hex(),
        "Compiling the same source twice should be fully deterministic"
    );
}
