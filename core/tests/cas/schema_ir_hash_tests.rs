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
fn an_aliased_source_hashes_identically_to_itself() {
    // Don't compare against a differently-worded literal source here - a
    // `FrozenUnit`'s `span` is part of its hash identity, and `UserId` vs
    // `u64` differ in byte length, so such a comparison would spuriously
    // fail despite correct erasure. Idempotence of the aliased form itself
    // is the right thing to assert.
    let source = "type UserId = u64\nstruct X {\n    id: UserId\n}\n";
    let a = IncrementalInterpreter::from_source(source);
    let b = IncrementalInterpreter::from_source(source);

    assert_eq!(schema_ir_hash(&a), schema_ir_hash(&b));
}

#[test]
fn changing_an_alias_target_moves_the_hash() {
    // Proves substitution is actually happening, not just name-dropping:
    // the alias declaration itself is erased either way, so if its target
    // didn't matter the hash wouldn't move.
    let before = IncrementalInterpreter::from_source(
        "type UserId = u64\nstruct X {\n    id: UserId\n}\n",
    );
    let after = IncrementalInterpreter::from_source(
        "type UserId = u32\nstruct X {\n    id: UserId\n}\n",
    );

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
