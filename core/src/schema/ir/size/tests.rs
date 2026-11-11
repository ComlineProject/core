//! The sizer on types written here: what the vectors don't cover (names and
//! imports, recursion at depth, where a format stops) and what they do, by
//! hand, in each format. The vectors are in `vectors.rs`.

use serde_json::{json, Value};

use super::*;
use crate::schema::ir::frozen::types::{PrimitiveType as P, TypeRef};
use crate::schema::ir::frozen::unit::{ConfigValue, FrozenUnit};

fn packed() -> &'static Sizes {
    rules("packed@1").unwrap()
}

fn msgpack() -> &'static Sizes {
    rules("msgpack@1").unwrap()
}

fn json() -> &'static Sizes {
    rules("json@1").unwrap()
}

fn prim(p: P) -> TypeRef {
    TypeRef::Primitive(p)
}

fn named(name: &str) -> TypeRef {
    TypeRef::Named(name.to_string())
}

fn array(element: TypeRef, length: Option<u64>, max: Option<u64>) -> TypeRef {
    TypeRef::Array { element: Box::new(element), length, max }
}

/// `char[max N]`.
fn chars(max: u64) -> TypeRef {
    array(prim(P::Char), None, Some(max))
}

/// `T[max N]`.
fn list(element: TypeRef, max: u64) -> TypeRef {
    array(element, None, Some(max))
}

fn field(name: &str, ty: TypeRef, optional: bool) -> FrozenUnit {
    FrozenUnit::Field {
        docstring: None,
        parameters: vec![],
        optional,
        name: name.to_string(),
        ty,
        default: None,
        span: (0, 0),
    }
}

fn depth(limit: i64) -> Vec<FrozenUnit> {
    vec![FrozenUnit::Property { name: "max_depth".to_string(), value: ConfigValue::Integer(limit) }]
}

fn structure(name: &str, fields: Vec<FrozenUnit>, max_depth: Option<i64>) -> FrozenUnit {
    FrozenUnit::Struct {
        docstring: None,
        parameters: max_depth.map(depth).unwrap_or_default(),
        name: name.to_string(),
        fields,
        span: (0, 0),
    }
}

fn unit_enum(name: &str, variants: &[&str]) -> FrozenUnit {
    FrozenUnit::Enum {
        docstring: None,
        name: name.to_string(),
        variants: variants.iter().map(|v| FrozenUnit::EnumVariant(v.to_string(), (0, 0))).collect(),
        span: (0, 0),
    }
}

fn alias(name: &str, target: TypeRef) -> FrozenUnit {
    FrozenUnit::TypeAlias {
        docstring: None,
        parameters: vec![],
        name: name.to_string(),
        target,
        span: (0, 0),
    }
}

fn schema(namespace: &str, mut units: Vec<FrozenUnit>) -> Vec<FrozenUnit> {
    units.insert(0, FrozenUnit::Namespace(namespace.to_string()));
    units
}

/// `ty`'s range in the one schema `units`.
fn in_schema(units: Vec<FrozenUnit>, ty: &TypeRef, rules: &Sizes) -> Option<Range> {
    let units = schema("test", units);
    let lookup = FrozenLookup::new([units.as_slice()]);
    range(ty, rules, &lookup, 0)
}

fn alone(ty: &TypeRef, rules: &Sizes) -> Option<Range> {
    in_schema(vec![], ty, rules)
}

fn between(min: u64, max: u64) -> Option<Range> {
    Some(Range { min, max: Some(max), upper_bound: false })
}

fn from(min: u64) -> Option<Range> {
    Some(Range { min, max: None, upper_bound: false })
}

fn bytes(ty: &TypeRef, rules: &Sizes) -> (u64, Option<u64>) {
    let r = alone(ty, rules).expect("a size");
    (r.min, r.max)
}

#[test]
fn a_bounded_type_is_finite_in_every_format() {
    let user = vec![structure(
        "User",
        vec![
            field("id", prim(P::U64), false),
            field("name", chars(32), false),
            field("tags", list(chars(8), 4), false),
            field("email", chars(64), true),
        ],
        None,
    )];
    for rules in [packed(), msgpack(), json()] {
        let got = in_schema(user.clone(), &named("User"), rules).unwrap();
        assert!(got.max.is_some_and(|max| max >= got.min), "{got:?}");
        assert!(!got.upper_bound);
    }
    // 8, a name of 1 to 130, tags of 1 to 133 and an email of 1 to 259.
    assert_eq!(in_schema(user, &named("User"), packed()), between(11, 530));
}

#[test]
fn an_unbounded_type_has_a_minimum_and_no_maximum() {
    for rules in [packed(), msgpack(), json()] {
        for ty in [prim(P::String), array(prim(P::U8), None, None), array(chars(4), None, None)] {
            let got = alone(&ty, rules).unwrap();
            assert_eq!(got.max, None, "{ty}");
            assert!(got.min > 0, "{ty}");
        }
    }
    // One unbounded field leaves the struct without a maximum.
    let s = vec![structure("S", vec![field("a", prim(P::U32), false), field("b", prim(P::String), false)], None)];
    assert_eq!(in_schema(s, &named("S"), packed()), from(5));
}

#[test]
fn scalars_are_the_rules() {
    assert_eq!(bytes(&prim(P::U32), packed()), (4, Some(4)));
    assert_eq!(bytes(&prim(P::U64), msgpack()), (1, Some(9)));
    assert_eq!(bytes(&prim(P::S64), json()), (1, Some(20)));
    assert_eq!(bytes(&prim(P::U128), msgpack()), (18, Some(18)));
    assert_eq!(bytes(&TypeRef::Unit, packed()), (0, Some(0)));
    assert_eq!(bytes(&TypeRef::Unit, json()), (4, Some(4)));
    assert_eq!(bytes(&prim(P::Char), json()), (3, Some(8)));
}

#[test]
fn a_string_is_a_header_on_its_bytes() {
    // The header tiers are applied to the bytes at each end: the least
    // is a header for none, the most a header for 4 a character.
    for (max_chars, most) in [(0, 1), (7, 29), (8, 34), (63, 254), (64, 259), (16383, 65535), (16384, 65541)] {
        assert_eq!(bytes(&chars(max_chars), msgpack()), (1, Some(most)), "char[max {max_chars}]");
    }
    // A fixed one has at least a header for its length.
    assert_eq!(bytes(&array(prim(P::Char), Some(8), None), msgpack()), (9, Some(34)));
    assert_eq!(bytes(&array(prim(P::Char), Some(32), None), msgpack()), (34, Some(130)));
    // A varint: 1 byte up to 127 bytes, then 2.
    assert_eq!(bytes(&chars(31), packed()), (1, Some(125)));
    assert_eq!(bytes(&chars(32), packed()), (1, Some(130)));
    assert_eq!(bytes(&chars(4095), packed()), (1, Some(16382)));
    assert_eq!(bytes(&chars(4096), packed()), (1, Some(16387)));
}

#[test]
fn msgpack_array_tiers_are_applied_to_the_count() {
    for (max, most) in [(0, 1), (15, 31), (16, 35), (65535, 3 + 2 * 65535), (65536, 5 + 2 * 65536)] {
        assert_eq!(bytes(&list(prim(P::U8), max), msgpack()), (1, Some(most)), "u8[max {max}]");
    }
    // A fixed one's count is written, and so its tier is at its length.
    assert_eq!(bytes(&array(prim(P::U8), Some(15), None), msgpack()), (16, Some(31)));
    assert_eq!(bytes(&array(prim(P::U8), Some(16), None), msgpack()), (19, Some(35)));
    // Where the header is `none`, it isn't written at all.
    assert_eq!(bytes(&array(prim(P::U8), Some(4), None), packed()), (4, Some(4)));
}

#[test]
fn a_struct_header_is_on_its_field_count() {
    let fields = |n: usize| (0..n).map(|i| field(&format!("f{i:02}"), prim(P::U8), false)).collect();
    for (n, least, most) in [(15, 16, 31), (16, 19, 35)] {
        let s = vec![structure("S", fields(n), None)];
        assert_eq!(in_schema(s, &named("S"), msgpack()), between(least, most), "{n} fields");
    }
}

#[test]
fn json_names_every_field_and_escapes_its_strings() {
    let s = vec![structure("Point", vec![field("x", prim(P::S32), false), field("y", prim(P::S32), false)], None)];
    // `{"x":0,"y":0}` to `{"x":-2147483648,"y":-2147483648}`.
    assert_eq!(in_schema(s, &named("Point"), json()), between(13, 33));
    // The worst case of a character is the six bytes of `\u0001`, a string's
    // quotes besides.
    for (max, most) in [(0, 2), (1, 8), (5, 32)] {
        assert_eq!(bytes(&chars(max), json()), (2, Some(most)), "char[max {max}]");
    }
    let worst = witnesses(&chars(3), json(), &NoNames, (), 100).unwrap();
    assert_eq!(worst.max, Some(json!({ "string": "\u{1}\u{1}\u{1}" })));
    // Where it is bytes of UTF-8, the worst case is the four-byte character.
    let worst = witnesses(&chars(3), packed(), &NoNames, (), 100).unwrap();
    assert_eq!(worst.max, Some(json!({ "string": "\u{1F600}\u{1F600}\u{1F600}" })));
    assert_eq!(worst.min, json!({ "string": "" }));
}

#[test]
fn an_optional_field_is_the_least_or_the_most_of_absent_and_present() {
    let s = |rules| {
        in_schema(
            vec![structure("S", vec![field("a", prim(P::U8), true)], None)],
            &named("S"),
            rules,
        )
    };
    // `packed@1`: a tag byte, then the value.
    assert_eq!(s(packed()), between(1, 2));
    // `msgpack@1`: `nil`, or the value alone: 1 byte, or 1 to 2.
    assert_eq!(s(msgpack()), between(2, 3));
    // `json@1`: `{"a":0}` to `{"a":null}`: `null` is 4 bytes, longer than any
    // number to 255 but not a longer one.
    assert_eq!(s(json()), between(7, 10));
}

#[test]
fn an_enum_is_its_shortest_to_its_longest_variant() {
    let color = vec![unit_enum("Color", &["Red", "Green", "Blue"])];
    assert_eq!(in_schema(color.clone(), &named("Color"), packed()), between(1, 1));
    assert_eq!(in_schema(color.clone(), &named("Color"), msgpack()), between(4, 6));
    assert_eq!(in_schema(color.clone(), &named("Color"), json()), between(5, 7));
    // The variant that is longest isn't the last: the witnesses say which.
    let w = witnesses(&named("Color"), json(), &FrozenLookup::new([schema("t", color).as_slice()]), 0, 10).unwrap();
    assert_eq!(w.min["unit_variant"]["variant"], "Red");
    assert_eq!(w.max.unwrap()["unit_variant"]["variant"], "Green");
    // 129 variants: an index past 127 is 2 bytes.
    let many: Vec<String> = (0..129).map(|i| format!("v{i:03}")).collect();
    let many: Vec<&str> = many.iter().map(String::as_str).collect();
    assert_eq!(in_schema(vec![unit_enum("Many", &many)], &named("Many"), packed()), between(1, 2));
}

#[test]
fn an_alias_is_what_it_names_across_schemas() {
    let types = schema("types", vec![alias("Name", chars(4)), structure("Card", vec![field("n", named("Name"), false)], None)]);
    let api = schema(
        "api",
        vec![
            FrozenUnit::Import("types::Name".to_string(), Some("Label".to_string()), (0, 0)),
            FrozenUnit::Import("types::Card".to_string(), None, (0, 0)),
            structure("Order", vec![field("label", named("Label"), false), field("card", named("Card"), false)], None),
        ],
    );
    let lookup = FrozenLookup::new([types.as_slice(), api.as_slice()]);
    let api_scope = lookup.scope_of("api").unwrap();
    // `Label` is `Name`, imported as another name: `char[max 4]`, twice.
    assert_eq!(range(&named("Label"), packed(), &lookup, api_scope), between(1, 17));
    assert_eq!(range(&named("Order"), packed(), &lookup, api_scope), between(2, 34));
    // `Name` isn't in `api`, but a qualified path finds it.
    assert_eq!(range(&named("types::Name"), packed(), &lookup, api_scope), between(1, 17));
}

#[test]
fn what_cant_be_sized_is_none() {
    let rules = [packed(), msgpack(), json()];
    for rules in rules {
        assert_eq!(alone(&TypeRef::Union(vec![prim(P::U8), prim(P::String)]), rules), None, "a union");
        assert_eq!(alone(&named("Missing"), rules), None, "a name that doesn't resolve");
        // An enum with fields holds values: its variants aren't names alone.
        let holding = FrozenUnit::Enum {
            docstring: None,
            name: "Shape".to_string(),
            variants: vec![FrozenUnit::EnumVariant("Dot".to_string(), (0, 0)), field("Circle", prim(P::F32), false)],
            span: (0, 0),
        };
        assert_eq!(in_schema(vec![holding], &named("Shape"), rules), None, "an enum with fields");
        assert_eq!(in_schema(vec![unit_enum("Never", &[])], &named("Never"), rules), None, "an enum with no variant");
        // A struct holding one has none either.
        let s = structure("S", vec![field("a", prim(P::U8), false), field("u", TypeRef::Union(vec![prim(P::U8)]), false)], None);
        assert_eq!(in_schema(vec![s], &named("S"), rules), None);
    }
    // A format with no `sizes`: a framing, a custom format.
    assert!(super::rules("jsonrpc@1").is_none());
    assert!(super::rules("datagram@3").is_none());
    assert!(super::rules("com.acme.bits@1").is_none());
    assert!(super::rules("jsonrpc").is_none());
    assert!(super::rules("packed@1").is_some());
}

/// A struct that contains itself.
fn node(next: TypeRef, optional: bool, max_depth: Option<i64>) -> Vec<FrozenUnit> {
    vec![structure("Node", vec![field("next", next, optional)], max_depth)]
}

#[test]
fn a_recursive_type_is_expanded_to_its_max_depth() {
    for (limit, packed_most) in [(1, 1), (2, 2), (3, 3), (10, 10)] {
        let units = node(named("Node"), true, Some(limit));
        assert_eq!(in_schema(units, &named("Node"), packed()), between(1, packed_most), "@max_depth = {limit}");
    }
    // `{"next":null}` is 13 bytes; each level around it is `{"next":` and `}`.
    for limit in 1..=4u64 {
        let units = node(named("Node"), true, Some(limit as i64));
        assert_eq!(in_schema(units, &named("Node"), json()), between(13, 13 + 9 * (limit - 1)), "@max_depth = {limit}");
    }
    // Where the field isn't optional there is no value at the end of it.
    let units = node(named("Node"), false, Some(3));
    assert_eq!(in_schema(units, &named("Node"), packed()), None);
    // An array is empty at the last level.
    let tree = vec![structure("Tree", vec![field("children", list(named("Tree"), 2), false)], Some(2))];
    assert_eq!(in_schema(tree, &named("Tree"), packed()), between(1, 3));
}

#[test]
fn a_recursive_type_without_a_limit_has_no_maximum() {
    let units = node(named("Node"), true, None);
    assert_eq!(in_schema(units, &named("Node"), packed()), from(1));
    let tree = vec![structure("Tree", vec![field("children", list(named("Tree"), 2), false)], None)];
    assert_eq!(in_schema(tree, &named("Tree"), msgpack()), from(2));
    // One that has to hold itself has no value at all.
    assert_eq!(in_schema(node(named("Node"), false, None), &named("Node"), packed()), None);
}

#[test]
fn each_struct_in_a_cycle_counts_its_own_depth() {
    let pair = |a: Option<i64>, b: Option<i64>| {
        vec![
            structure("A", vec![field("b", named("B"), true)], a),
            structure("B", vec![field("a", named("A"), true)], b),
        ]
    };
    // A, B, A, B, and then each is at its second level.
    assert_eq!(in_schema(pair(Some(2), Some(2)), &named("A"), packed()), between(1, 4));
    // With no limit on one of them, nothing ends it.
    assert_eq!(in_schema(pair(Some(2), None), &named("A"), packed()), from(1));
    assert_eq!(in_schema(pair(None, None), &named("A"), packed()), from(1));
    // A limit of 0 admits no value: the field is absent, and a required one couldn't be.
    assert_eq!(in_schema(node(named("Node"), true, Some(0)), &named("Node"), packed()), None);
}

#[test]
fn a_deep_branching_type_is_sized_once_for_each_level() {
    // Eight children, twenty deep: 8^19 leaves, which a walk of every branch
    // would never finish; each level is sized once.
    let tree = |limit| vec![structure("Tree", vec![field("kids", list(named("Tree"), 8), false)], Some(limit))];
    let mut most = 1u64;
    for _ in 1..20 {
        most = 1 + 8 * most;
    }
    assert_eq!(in_schema(tree(20), &named("Tree"), packed()), between(1, most));
    // At a hundred, the most overflows 64 bits: no bound, and the least stays.
    assert_eq!(in_schema(tree(100), &named("Tree"), packed()), from(1));
    // Too deep to expand at all.
    assert_eq!(in_schema(tree(1000), &named("Tree"), packed()), None);
}

#[test]
fn where_the_formats_stop() {
    // A bound above what a header can say is cut to what can be written.
    assert_eq!(bytes(&list(prim(P::U8), 4_294_967_295), packed()), (1, Some(5 + 4_294_967_295)));
    assert_eq!(bytes(&list(prim(P::U8), 5_000_000_000), packed()), (1, Some(5 + 4_294_967_295)));
    // 4 bytes a character: as many characters as the length can hold.
    let most = 5 + (4_294_967_295 / 4) * 4;
    assert_eq!(bytes(&chars(4_294_967_295), msgpack()), (1, Some(most)));
    assert_eq!(bytes(&chars(u64::MAX), msgpack()), (1, Some(most)));
    // A header that says no count has no such limit.
    assert_eq!(bytes(&chars(4_294_967_295), json()), (2, Some(2 + 6 * 4_294_967_295)));
    assert_eq!(bytes(&array(prim(P::U8), Some(5_000_000_000), None), packed()), (5_000_000_000, Some(5_000_000_000)));
    // A fixed count or length it would have to write can't be.
    assert_eq!(alone(&array(prim(P::U8), Some(5_000_000_000), None), msgpack()), None);
    assert_eq!(alone(&array(prim(P::Char), Some(5_000_000_000), None), msgpack()), None);
    assert_eq!(alone(&array(prim(P::Char), Some(5_000_000_000), None), packed()), None);
    // Beyond a `u64`, a maximum has no number, and a minimum can't be written.
    let big = list(list(list(prim(P::U64), 4_294_967_295), 4_294_967_295), 4_294_967_295);
    assert_eq!(alone(&big, packed()), from(1));
    let huge = array(array(array(prim(P::U64), Some(4_294_967_295), None), Some(4_294_967_295), None), Some(4_294_967_295), None);
    assert_eq!(alone(&huge, packed()), None);
    assert_eq!(alone(&chars(u64::MAX), json()), from(2));
}

struct NoNames;

impl Lookup for NoNames {
    type Scope = ();

    fn resolve(&self, _: (), _: &str) -> Option<Resolved<'_, ()>> {
        None
    }
}

fn witness(ty: &TypeRef, rules: &Sizes, units: Vec<FrozenUnit>) -> Witnesses {
    let units = schema("test", units);
    let lookup = FrozenLookup::new([units.as_slice()]);
    witnesses(ty, rules, &lookup, 0, 1000).expect("witnesses")
}

#[test]
fn witnesses_are_the_least_and_the_most_value() {
    let point = vec![structure("Point", vec![field("x", prim(P::S32), false), field("y", prim(P::S32), false)], None)];
    let got = witness(&named("Point"), json(), point);
    let expected = |v: i64| {
        json!({ "struct": { "name": "Point", "fields": [["x", { "i32": v }], ["y", { "i32": v }]] } })
    };
    assert_eq!(got.min, expected(0));
    assert_eq!(got.max, Some(expected(i32::MIN as i64)));

    // JSON writes `null` in 4 bytes, so a short number is the least and `null` is the most.
    let got = witness(&named("S"), json(), vec![structure("S", vec![field("a", prim(P::U8), true)], None)]);
    assert_eq!(got.min["struct"]["fields"][0][1], json!({ "option": { "u8": 0 } }));
    assert_eq!(got.max.unwrap()["struct"]["fields"][0][1], json!({ "option": null }));
    // Where the tag is a byte of its own, the least is absent and the most present.
    let got = witness(&named("S"), packed(), vec![structure("S", vec![field("a", prim(P::U8), true)], None)]);
    assert_eq!(got.min["struct"]["fields"][0][1], json!({ "option": null }));
    assert_eq!(got.max.unwrap()["struct"]["fields"][0][1], json!({ "option": { "u8": 255 } }));

    // Arrays: the least is empty, the most full of the most.
    let got = witness(&list(prim(P::U8), 3), msgpack(), vec![]);
    assert_eq!(got.min, json!({ "array": [] }));
    assert_eq!(got.max, Some(json!({ "array": [{ "u8": 255 }, { "u8": 255 }, { "u8": 255 }] })));
    // A fixed one is a tuple, of its length at least.
    let got = witness(&array(prim(P::Bool), Some(2), None), json(), vec![]);
    assert_eq!(got.min, json!({ "tuple": [{ "bool": true }, { "bool": true }] }));
    assert_eq!(got.max, Some(json!({ "tuple": [{ "bool": false }, { "bool": false }] })));
    // Index to the last variant.
    let got = witness(&named("E"), packed(), vec![unit_enum("E", &["A", "B", "C"])]);
    assert_eq!(got.min, json!({ "unit_variant": { "enum": "E", "index": 0, "variant": "A" } }));
    assert_eq!(got.max, Some(json!({ "unit_variant": { "enum": "E", "index": 2, "variant": "C" } })));
}

#[test]
fn a_witness_is_left_out_where_there_is_nothing_to_build() {
    // No maximum: no value of it.
    let got = witness(&prim(P::String), packed(), vec![]);
    assert_eq!((got.min, got.max), (json!({ "string": "" }), None));
    // Too large to write down: the range is there, and the least value.
    let ty = chars(4_000_000_000);
    let got = witnesses(&ty, msgpack(), &NoNames, (), 1000).unwrap();
    assert_eq!(got.min, json!({ "string": "" }));
    assert_eq!(got.max, None);
    assert!(range(&ty, msgpack(), &NoNames, ()).is_some_and(|r| r.max.is_some()));
    // And where even the least is: a fixed string of a billion.
    assert_eq!(witnesses(&array(prim(P::Char), Some(1_000_000_000), None), packed(), &NoNames, (), 1000), None);
    // A type with no size has no witness.
    assert_eq!(witnesses(&TypeRef::Union(vec![]), packed(), &NoNames, (), 1000), None);
}

#[test]
fn a_witness_follows_the_range_of_what_it_holds() {
    // A struct of an optional string and a fixed array: the witnesses are built
    // by the code that computes the range, so a count in one is the count in the other.
    let s = vec![structure(
        "S",
        vec![field("name", chars(2), true), field("tags", array(prim(P::U8), Some(2), None), false)],
        None,
    )];
    let got = witness(&named("S"), json(), s.clone());
    let tags = |w: &Value| w["struct"]["fields"][1][1]["tuple"].as_array().unwrap().len();
    assert_eq!(tags(&got.min), 2);
    assert_eq!(tags(&got.max.unwrap()), 2);
    // `{"name":"","tags":[0,0]}` to `{"name":"\u0001\u0001","tags":[255,255]}`.
    assert_eq!(in_schema(s, &named("S"), json()), between(24, 40));
}
