// Every frozen unit core produces survives the CAS's CBOR encoding unchanged.
//
// The single-unit round-trip tests in `schema_integration_tests.rs` build
// their units by hand; this compiles real packages instead, so a change to
// the frozen IR's shape is checked against everything the compiler actually
// emits - std, the example and fixture packages, and a schema that touches
// every construct the grammar has.

use std::path::Path;

use comline_core::package::build::{compile_package, PackageSources};
use comline_core::package::config::ir::context::ProjectContext;
use comline_core::schema::ir::frozen::cas::blob::{
    blob_to_frozen_unit, frozen_unit_to_blob, schema_ir_hash,
};
use comline_core::schema::ir::frozen::unit::FrozenUnit;

/// A schema using every declaration kind and type shape the grammar has.
const TOUR: &str = r#"
use std::validators::StringBounds

/// Limits.
const MAX_NAME: u32 = 64
const GREETING: string = "hello"
const STRICT: bool = true

type UserId = u64
type Names = string[]

validator Range for u8, u32 {
    min: u32 = 0
    max: u32 = 100
    validate {
        assert(value >= params.min and value <= params.max, "{value.name} is out of range")
    }
}

enum Role {
    Admin
    Member
}

@deprecated = "use User"
struct LegacyUser {
    id: UserId
}

settings Strict {
    validation.unknown = "error"
}

@settings = Strict
struct User {
    /// The user's id.
    id: UserId
    @validators = [StringBounds(min_chars = 1, max_chars = 32)]
    name: string
    @validators = [Range(min = 1, max = 99)]
    age: u8 = 18
    optional nickname: string
    role: Role
    tags: Names
    scores: u32[]
    digest: u8[32]
    grid: s16[][]
    ratio: f64
    weight: f32 = 1
    either: union(string u32)
    flag: bool = false
}

error NotFound {
    message = "user {id} not found"
    id: UserId
}

error Denied {
    message = "denied"
}

protocol Users {
    /// Look a user up.
    @timeout_ms = 500
    function get(id: UserId) -> User ! NotFound, Denied;
    function ping() -> ();
    @idempotent
    function forget(UserId);
}
"#;

fn units_of(context: &ProjectContext) -> Vec<(String, Vec<FrozenUnit>)> {
    context
        .schema_contexts
        .iter()
        .map(|schema| {
            let schema = schema.borrow();
            let units = schema
                .frozen_schema
                .borrow()
                .clone()
                .unwrap_or_else(|| panic!("{:?} has no frozen units", schema.namespace));
            (schema.namespace.join("::"), units)
        })
        .collect()
}

fn assert_round_trips(label: &str, context: &ProjectContext) {
    let schemas = units_of(context);
    assert!(!schemas.is_empty(), "{label}: compiled no schemas");
    for (namespace, units) in schemas {
        let restored: Vec<FrozenUnit> = units
            .iter()
            .map(|unit| {
                let blob = frozen_unit_to_blob(unit)
                    .unwrap_or_else(|e| panic!("{label} {namespace}: encode {unit:?}: {e}"));
                blob_to_frozen_unit(&blob)
                    .unwrap_or_else(|e| panic!("{label} {namespace}: decode {unit:?}: {e}"))
            })
            .collect();
        assert_eq!(restored, units, "{label} {namespace}: a unit changed through CBOR");
        assert_eq!(
            schema_ir_hash(&restored),
            schema_ir_hash(&units),
            "{label} {namespace}: the IR hash changed through CBOR"
        );
    }
}

#[test]
fn every_construct_round_trips_through_cbor() {
    let context = PackageSources::new()
        .schema(["tour"], TOUR)
        .compile()
        .expect("the tour schema compiles");
    // `std::validators` is merged in alongside, so std round-trips too.
    assert!(context.schema_contexts.len() >= 2);
    assert_round_trips("tour", &context);
}

#[test]
fn the_shipped_packages_round_trip_through_cbor() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for package in ["examples/imports", "tests/fixtures/packages/test"] {
        let context = compile_package(&root.join(package))
            .unwrap_or_else(|e| panic!("{package} compiles: {e}"));
        assert_round_trips(package, &context);
    }
}

/// `f32`/`f64` once froze to the text `"float"`, which validation didn't know
/// as a primitive - so any float field failed with "Unknown type 'float'".
#[test]
fn float_fields_validate() {
    PackageSources::new()
        .schema(["floats"], "struct S {\n    x: f32\n    y: f64\n}\n")
        .compile()
        .expect("f32 and f64 are primitives");
}
