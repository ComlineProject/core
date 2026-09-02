// `schema_ir_hash` — the canonical frozen-schema digest a generator embeds as
// the connection handshake's `IR_HASH`. It must be deterministic for a given
// frozen IR and move when the schema's meaning (or, like the CAS blob hash,
// its formatting) changes.

use comline_core::schema::ir::compiler::interpreter::incremental::IncrementalInterpreter;
use comline_core::schema::ir::compiler::Compile;
use comline_core::schema::ir::frozen::schema_ir_hash;

#[test]
fn identical_source_hashes_identically() {
    let source = "struct User {\n    id: u64\n    name: str\n}\n";
    let a = IncrementalInterpreter::from_source(source);
    let b = IncrementalInterpreter::from_source(source);

    assert_eq!(schema_ir_hash(&a), schema_ir_hash(&b));
}

#[test]
fn a_semantic_change_moves_the_hash() {
    let before = IncrementalInterpreter::from_source("struct User {\n    id: u64\n}\n");
    let after = IncrementalInterpreter::from_source("struct User {\n    id: u64\n    name: str\n}\n");

    assert_ne!(schema_ir_hash(&before), schema_ir_hash(&after));
}

#[test]
fn unit_order_is_significant() {
    let ab = IncrementalInterpreter::from_source(
        "struct A {\n    x: u64\n}\nstruct B {\n    y: u64\n}\n",
    );
    let ba = IncrementalInterpreter::from_source(
        "struct B {\n    y: u64\n}\nstruct A {\n    x: u64\n}\n",
    );

    // Different declaration order ⇒ different frozen IR ⇒ different identity.
    assert_ne!(schema_ir_hash(&ab), schema_ir_hash(&ba));
}

#[test]
fn an_empty_schema_still_hashes() {
    let empty: Vec<comline_core::schema::ir::frozen::unit::FrozenUnit> = Vec::new();
    // Just needs to not panic and be stable.
    assert_eq!(schema_ir_hash(&empty), schema_ir_hash(&empty));
}
