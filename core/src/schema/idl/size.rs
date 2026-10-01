//! A theoretical, codec-independent "wire size" for a Comline type: each
//! primitive's natural raw/fixed-width-binary size, fixed arrays as
//! `width × N`, and an explicit [`WireSize::Variable`] for anything
//! genuinely unbounded (`string`, a dynamic array, or anything containing
//! one) — never an invented number for those.
//!
//! This is **not** tied to any actual wire codec: Comline's runtime encoding
//! is pluggable (JSON, MessagePack, or a raw codec — see
//! `docs/guide/runtime/call-system.md`), and e.g. MessagePack's own integers
//! are variable-width by *value*, not by declared type. This module answers
//! a different, simpler question — "if this were packed with no padding and
//! no framing overhead, what's the smallest size a fixed-width field could
//! take" — useful for an at-a-glance hover estimate, not a protocol
//! commitment.
//!
//! Deliberately operates on the raw AST (`super::grammar`), not the frozen
//! IR (`crate::schema::ir::frozen`): freezing already collapses `f32`/`f64`
//! into one string (`"float"`) and drops a fixed array's literal size
//! entirely (`Type[8]` and `Type[]` both become `"{elem}[]"`) — exactly the
//! precision this module needs. See `ir/validation/validator.rs`'s
//! `detect_cycle` for what that loss already breaks: its `ends_with("[]")`
//! cycle-escape matches *every* array, fixed or dynamic, because by the
//! time it runs the distinction is gone — a fixed-size self-reference like
//! `struct Node { next: Node[4] }` slips through uncaught. This module does
//! not reuse that cycle guard; it has its own (see [`size_of_struct`]).

use std::collections::HashSet;

use super::grammar::{self, Type};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireSize {
    /// A known, fixed number of bytes.
    Fixed(u32),
    /// Genuinely unbounded: a string, a dynamic array, or anything
    /// containing one.
    Variable,
    /// Couldn't compute a number, and it isn't because the type is
    /// unbounded: an unresolved name (a typo, or a struct not yet declared
    /// in a mid-edit buffer) or a detected cycle. Distinct from `Variable`
    /// so a caller doesn't misreport a dangling reference as "this field is
    /// a string" when it's actually "this field doesn't resolve."
    Unknown,
}

/// What a bare type name resolved to, for [`SizeLookup`].
pub enum SizeTarget<'a> {
    Struct(&'a grammar::Struct),
    Enum(&'a grammar::Enum),
}

/// Resolves a bare (last-segment) type name to its declaration. Deliberately
/// left to the caller: this module has no opinion on *how* a name should be
/// resolved across files (a flat scan, a `use`-scoped resolver, …) — only on
/// what to do once it is.
pub trait SizeLookup {
    fn resolve(&self, bare_name: &str) -> Option<SizeTarget<'_>>;
}

/// The size of a standalone type (a const's type, a function arg/return
/// type, …). For a *field*, remember to check `Field::optional()` first — a
/// raw encoding needs a presence sentinel for an optional field regardless
/// of its underlying type's own fixedness, so an optional field is always
/// [`WireSize::Variable`] here; that check isn't (and can't be) folded into
/// this function since `optional` lives on `Field`, not `Type`.
pub fn size_of_type(ty: &Type, lookup: &impl SizeLookup) -> WireSize {
    let mut visiting = HashSet::new();
    size_of_type_inner(ty, lookup, &mut visiting)
}

/// The total size of a struct: the sum of its fields, `Variable`/`Unknown`
/// if any field is. Also the entry point a hovered struct declaration
/// itself should use (not just a `Named` reference to one elsewhere) — it
/// seeds the cycle guard with this struct before summing, so a struct that
/// refers back to itself (directly or through others) is caught here too,
/// not only when reached via someone else's field.
pub fn size_of_struct(s: &grammar::Struct, lookup: &impl SizeLookup) -> WireSize {
    let mut visiting = HashSet::new();
    size_of_struct_inner(s, lookup, &mut visiting)
}

fn size_of_struct_inner(
    s: &grammar::Struct,
    lookup: &impl SizeLookup,
    visiting: &mut HashSet<usize>,
) -> WireSize {
    let key = s as *const _ as usize;
    if !visiting.insert(key) {
        return WireSize::Unknown; // cycle
    }

    let mut total: u32 = 0;
    let mut result = WireSize::Fixed(0);
    for field in s.fields() {
        let field_size = if field.optional() {
            WireSize::Variable
        } else {
            size_of_type_inner(field.field_type(), lookup, visiting)
        };
        match field_size {
            WireSize::Fixed(w) => total = total.saturating_add(w),
            v @ (WireSize::Variable | WireSize::Unknown) => {
                result = v;
                break;
            }
        }
    }

    visiting.remove(&key);

    match result {
        WireSize::Fixed(_) => WireSize::Fixed(total),
        other => other,
    }
}

/// Discriminant width for a plain (no associated data) enum variant list.
fn enum_size(e: &grammar::Enum) -> WireSize {
    WireSize::Fixed(if e.variants().len() > 256 { 2 } else { 1 })
}

fn size_of_type_inner(ty: &Type, lookup: &impl SizeLookup, visiting: &mut HashSet<usize>) -> WireSize {
    match ty {
        Type::S8(_) | Type::U8(_) | Type::Bool(_) => WireSize::Fixed(1),
        Type::S16(_) | Type::U16(_) => WireSize::Fixed(2),
        Type::S32(_) | Type::U32(_) | Type::F32(_) => WireSize::Fixed(4),
        Type::S64(_) | Type::U64(_) | Type::F64(_) => WireSize::Fixed(8),
        Type::Str(_) | Type::String(_) => WireSize::Variable,
        Type::Unit(_) => WireSize::Fixed(0),

        Type::Array(arr) => {
            let elem = size_of_type_inner(arr.elem_type(), lookup, visiting);
            match (&arr.size, elem) {
                (None, _) => WireSize::Variable, // Type[] — always unbounded
                (Some(_), WireSize::Variable) => WireSize::Variable, // Type[N] of a variable elem is still unbounded
                (Some(_), WireSize::Unknown) => WireSize::Unknown,
                (Some(n), WireSize::Fixed(w)) => {
                    // `IntegerLiteral`'s grammar pattern is `-?\d+`, reused
                    // generically for array sizes — a negative or absurdly
                    // large literal is syntactically possible from a
                    // mid-edit buffer, so this must never panic.
                    match u32::try_from(n.value()).ok().and_then(|n| w.checked_mul(n)) {
                        Some(total) => WireSize::Fixed(total),
                        None => WireSize::Unknown,
                    }
                }
            }
        }

        Type::Union(u) => {
            let sizes: Vec<WireSize> = u
                .members()
                .iter()
                .map(|m| size_of_type_inner(m, lookup, visiting))
                .collect();
            if sizes.iter().any(|s| matches!(s, WireSize::Variable)) {
                WireSize::Variable
            } else if sizes.iter().any(|s| matches!(s, WireSize::Unknown)) {
                WireSize::Unknown
            } else {
                let max = sizes
                    .iter()
                    .filter_map(|s| match s {
                        WireSize::Fixed(w) => Some(*w),
                        _ => None,
                    })
                    .max()
                    .unwrap_or(0);
                // 1-byte discriminant + largest member — a tagged-union
                // convention this module defines, not one drawn from any
                // existing codec (none of them type-map `union(...)` yet).
                WireSize::Fixed(1 + max)
            }
        }

        Type::Named(id) => {
            let bare = id.text.rsplit("::").next().unwrap_or(&id.text);
            match lookup.resolve(bare) {
                None => WireSize::Unknown,
                Some(SizeTarget::Enum(e)) => enum_size(e),
                Some(SizeTarget::Struct(s)) => size_of_struct_inner(s, lookup, visiting),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::grammar::Declaration;

    /// A flat, project-wide lookup across however many parsed documents are
    /// handed to it — mirrors the simplification `language-server`'s hover
    /// uses for cross-file resolution (first match wins, no `use`-scoping),
    /// and lets one lookup impl stand in for either a single file or a
    /// multi-file project in these tests.
    struct TestLookup<'a> {
        docs: Vec<&'a grammar::Document>,
    }

    impl<'a> SizeLookup for TestLookup<'a> {
        fn resolve(&self, bare_name: &str) -> Option<SizeTarget<'_>> {
            for doc in &self.docs {
                for decl in &doc.0 {
                    match &**decl {
                        Declaration::Struct(s) if s.name() == bare_name => {
                            return Some(SizeTarget::Struct(s));
                        }
                        Declaration::Enum(e) if e.name() == bare_name => {
                            return Some(SizeTarget::Enum(e));
                        }
                        _ => {}
                    }
                }
            }
            None
        }
    }

    fn parse(src: &str) -> grammar::Document {
        grammar::parse(src).expect("fixture should parse")
    }

    fn only_type(doc: &grammar::Document) -> &Type {
        // Pull the one field's type out of the one struct in `doc` — the
        // shape every "a single type" test fixture below uses.
        for decl in &doc.0 {
            if let Declaration::Struct(s) = &**decl {
                return s.fields()[0].field_type();
            }
        }
        panic!("fixture has no struct");
    }

    fn only_struct(doc: &grammar::Document) -> &grammar::Struct {
        for decl in &doc.0 {
            if let Declaration::Struct(s) = &**decl {
                return s;
            }
        }
        panic!("fixture has no struct");
    }

    fn empty_lookup() -> TestLookup<'static> {
        TestLookup { docs: vec![] }
    }

    #[test]
    fn primitive_widths() {
        let cases = [
            ("s8", 1), ("u8", 1), ("bool", 1),
            ("s16", 2), ("u16", 2),
            ("s32", 4), ("u32", 4), ("f32", 4),
            ("s64", 8), ("u64", 8), ("f64", 8),
        ];
        for (ty, bytes) in cases {
            let doc = parse(&format!("struct S {{ field: {ty} }}"));
            assert_eq!(
                size_of_type(only_type(&doc), &empty_lookup()),
                WireSize::Fixed(bytes),
                "type {ty}"
            );
        }
    }

    #[test]
    fn strings_are_variable() {
        for ty in ["str", "string"] {
            let doc = parse(&format!("struct S {{ field: {ty} }}"));
            assert_eq!(size_of_type(only_type(&doc), &empty_lookup()), WireSize::Variable);
        }
    }

    #[test]
    fn unit_is_zero_bytes() {
        let doc = parse("struct S { field: () }");
        assert_eq!(size_of_type(only_type(&doc), &empty_lookup()), WireSize::Fixed(0));
    }

    #[test]
    fn fixed_array_of_fixed_element() {
        let doc = parse("struct S { field: u32[4] }");
        assert_eq!(size_of_type(only_type(&doc), &empty_lookup()), WireSize::Fixed(16));
    }

    #[test]
    fn fixed_count_of_variable_elements_is_still_variable() {
        let doc = parse("struct S { field: string[4] }");
        assert_eq!(size_of_type(only_type(&doc), &empty_lookup()), WireSize::Variable);
    }

    #[test]
    fn dynamic_array_is_variable_regardless_of_element() {
        let doc = parse("struct S { field: u32[] }");
        assert_eq!(size_of_type(only_type(&doc), &empty_lookup()), WireSize::Variable);
    }

    #[test]
    fn negative_array_size_is_unknown_not_a_panic() {
        // `IntegerLiteral`'s grammar pattern is `-?\d+`, reused generically
        // for array sizes — a mid-edit buffer can contain this.
        let doc = parse("struct S { field: u32[-1] }");
        assert_eq!(size_of_type(only_type(&doc), &empty_lookup()), WireSize::Unknown);
    }

    #[test]
    fn overflowing_array_size_is_unknown_not_a_panic() {
        let doc = parse("struct S { field: u8[99999999999] }");
        assert_eq!(size_of_type(only_type(&doc), &empty_lookup()), WireSize::Unknown);
    }

    #[test]
    fn unresolved_named_type_is_unknown() {
        let doc = parse("struct S { field: Missing }");
        assert_eq!(size_of_type(only_type(&doc), &empty_lookup()), WireSize::Unknown);
    }

    #[test]
    fn nested_struct_sums_correctly() {
        let doc = parse("struct Inner { a: u32 b: u8 } struct Outer { inner: Inner }");
        let lookup = TestLookup { docs: vec![&doc] };
        assert_eq!(size_of_type(only_type_named(&doc, "Outer"), &lookup), WireSize::Fixed(5));
    }

    #[test]
    fn any_variable_field_makes_the_whole_struct_variable() {
        let doc = parse("struct S { a: u32 b: string }");
        let lookup = TestLookup { docs: vec![&doc] };
        assert_eq!(size_of_struct(only_struct(&doc), &lookup), WireSize::Variable);
    }

    #[test]
    fn optional_field_is_variable_even_over_a_fixed_type() {
        let doc = parse("struct S { optional a: u32 }");
        let lookup = TestLookup { docs: vec![&doc] };
        assert_eq!(size_of_struct(only_struct(&doc), &lookup), WireSize::Variable);
    }

    #[test]
    fn enum_size_is_a_discriminant_byte() {
        let doc = parse("enum E { A B C }");
        for decl in &doc.0 {
            if let Declaration::Enum(e) = &**decl {
                assert_eq!(enum_size(e), WireSize::Fixed(1));
            }
        }
    }

    #[test]
    fn enum_size_widens_past_256_variants() {
        let variants: String = (0..300).map(|i| format!("V{i} ")).collect();
        let doc = parse(&format!("enum E {{ {variants} }}"));
        for decl in &doc.0 {
            if let Declaration::Enum(e) = &**decl {
                assert_eq!(enum_size(e), WireSize::Fixed(2));
            }
        }
    }

    #[test]
    fn union_of_fixed_members_is_discriminant_plus_max() {
        let doc = parse("struct S { field: union(u8 u64) }");
        assert_eq!(size_of_type(only_type(&doc), &empty_lookup()), WireSize::Fixed(1 + 8));
    }

    #[test]
    fn union_with_a_variable_member_is_variable() {
        let doc = parse("struct S { field: union(u8 string) }");
        assert_eq!(size_of_type(only_type(&doc), &empty_lookup()), WireSize::Variable);
    }

    #[test]
    fn direct_self_reference_via_fixed_array_is_unknown_not_a_hang() {
        // This is precisely the case `ir::validation::validator`'s
        // `detect_cycle` currently mishandles: frozen IR has already
        // dropped the `[4]` literal by the time that check runs, so a
        // fixed-size self-reference slips past its `ends_with("[]")`
        // cycle-escape. This module must not trust that check — and must
        // not hang on this shape either.
        let doc = parse("struct Node { next: Node[4] }");
        let lookup = TestLookup { docs: vec![&doc] };
        assert_eq!(size_of_struct(only_struct(&doc), &lookup), WireSize::Unknown);
    }

    #[test]
    fn mutual_two_struct_cycle_is_unknown() {
        let doc = parse("struct A { b: B } struct B { a: A }");
        let lookup = TestLookup { docs: vec![&doc] };
        assert_eq!(size_of_type(only_type_named(&doc, "A"), &lookup), WireSize::Unknown);
    }

    #[test]
    fn cross_file_cycle_is_unknown_not_a_hang() {
        // `A` (file 1) refers to `B` (file 2), which refers back to `A` —
        // nothing in the codebase guards this shape today (the validator's
        // own cycle check is scoped to one file's units).
        let file1 = parse("struct A { b: B }");
        let file2 = parse("struct B { a: A }");
        let lookup = TestLookup { docs: vec![&file1, &file2] };
        assert_eq!(size_of_type(only_type_named(&file1, "A"), &lookup), WireSize::Unknown);
    }

    fn only_type_named<'a>(doc: &'a grammar::Document, name: &str) -> &'a Type {
        for decl in &doc.0 {
            if let Declaration::Struct(s) = &**decl {
                if s.name() == name {
                    return s.fields()[0].field_type();
                }
            }
        }
        panic!("no struct named {name}");
    }
}
