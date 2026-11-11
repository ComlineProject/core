//! The bytes a type takes in a format, for what an editor has on hand
//! mid-edit: the grammar a half-written file parses to, rather than the
//! frozen IR.
//!
//! There is one sizer, [`crate::schema::ir::size`]. Each type here becomes the
//! [`TypeRef`] the compiler would freeze it to
//! ([`TypeRef::from_grammar_sized`]) and goes through it, so what the editor
//! shows is what a build computes, from the same rules. What's different is
//! only where the names come from: [`SizeLookup`] resolves a bare name to a
//! declaration in the grammar, however the caller sees fit (a flat scan, a
//! `use`-scoped resolver, ...), as it has always done.
//!
//! What a name or a constant doesn't resolve to is a size that can't be told
//! (`None`), not a number: a struct not yet declared in a mid-edit buffer,
//! or `char[max MAX_NAME]` with `MAX_NAME` unknown, isn't reported as zero.
//! A struct that contains itself has no `@max_depth` here unless it writes
//! one (settings' `bounds.depth.max` isn't in the grammar), so its maximum is
//! unbounded.
//!
//! The format's rules are [`rules`]: `rules("packed@1")`. A caller shows one
//! range per format, each `None` as "unknown".

use std::borrow::Cow;
use std::cell::Cell;

use super::grammar::{self, Type};
use crate::schema::ir::frozen::types::TypeRef;
use crate::schema::ir::size::{self, Decl, Lookup, Member, Resolved};

pub use crate::schema::ir::size::{rules, Range, Sizes};

/// What a bare type name resolved to, for [`SizeLookup`].
pub enum SizeTarget<'a> {
    Struct(&'a grammar::Struct),
    Enum(&'a grammar::Enum),
    /// A `type` alias's own target type - recurse into it, same as if the
    /// field had been written with that type directly (aliases are fully
    /// transparent; see `compiler::alias_resolution` in the IR compiler).
    Alias(&'a Type),
}

/// Resolves a bare (last-segment) type name to its declaration. Deliberately
/// left to the caller: this module has no opinion on *how* a name should be
/// resolved across files (a flat scan, a `use`-scoped resolver, …) — only on
/// what to do once it is.
pub trait SizeLookup {
    fn resolve(&self, bare_name: &str) -> Option<SizeTarget<'_>>;
}

/// The range of a standalone type (a const's type, a function arg/return
/// type, …). For a *field*, use [`range_of_field`]: whether it is `optional`
/// is on the field, not the type.
///
/// `constant` gives the value of a constant a size names
/// (`char[max MAX_NAME]`); `&|_| None` where there is none, which makes such a
/// type unsized.
pub fn range_of_type(
    ty: &Type,
    rules: &Sizes,
    lookup: &impl SizeLookup,
    constant: &dyn Fn(&str) -> Option<u64>,
) -> Option<Range> {
    let flat = Flat::new(lookup, constant);
    let ty = flat.freeze(ty);
    let range = size::range(&ty, rules, &flat, ())?;
    flat.complete(range)
}

/// The range of a field: its type, or absent when it is `optional`.
pub fn range_of_field(
    field: &grammar::Field,
    rules: &Sizes,
    lookup: &impl SizeLookup,
    constant: &dyn Fn(&str) -> Option<u64>,
) -> Option<Range> {
    let flat = Flat::new(lookup, constant);
    let ty = flat.freeze(field.field_type());
    let range = if field.optional() {
        size::range_optional(&ty, rules, &flat, ())?
    } else {
        size::range(&ty, rules, &flat, ())?
    };
    flat.complete(range)
}

/// The range of a struct: also the entry point a hovered struct declaration
/// itself should use (not just a `Named` reference to one elsewhere), so a
/// struct that refers back to itself is a recursion from its own start.
pub fn range_of_struct(
    s: &grammar::Struct,
    rules: &Sizes,
    lookup: &impl SizeLookup,
    constant: &dyn Fn(&str) -> Option<u64>,
) -> Option<Range> {
    let flat = Flat::new(lookup, constant);
    let resolved = flat.resolved_struct(s);
    let range = size::range_of_decl(resolved, rules, &flat)?;
    flat.complete(range)
}

/// The range of an enum's own declaration, symmetric with [`range_of_struct`].
pub fn range_of_enum(e: &grammar::Enum, rules: &Sizes) -> Option<Range> {
    struct NoNames;
    impl Lookup for NoNames {
        type Scope = ();
        fn resolve(&self, _: (), _: &str) -> Option<Resolved<'_, ()>> {
            None
        }
    }
    let variants = e.variants().iter().map(|v| Cow::Owned(v.name.text.clone())).collect();
    let decl = Decl::Enum { name: Cow::Owned(e.name()), variants };
    size::range_of_decl(Resolved { key: e as *const grammar::Enum as usize, scope: (), decl }, rules, &NoNames)
}

/// A [`SizeLookup`] as the sizer's [`Lookup`]: by bare name, with the types
/// it holds frozen the way the compiler would.
struct Flat<'a, L> {
    lookup: &'a L,
    constant: &'a dyn Fn(&str) -> Option<u64>,
    /// A constant a size named didn't resolve.
    unresolved: Cell<bool>,
}

impl<'a, L: SizeLookup> Flat<'a, L> {
    fn new(lookup: &'a L, constant: &'a dyn Fn(&str) -> Option<u64>) -> Self {
        Flat { lookup, constant, unresolved: Cell::new(false) }
    }

    fn freeze(&self, ty: &Type) -> TypeRef {
        TypeRef::from_grammar_sized(ty, &|name| {
            let value = (self.constant)(name);
            if value.is_none() {
                self.unresolved.set(true);
            }
            value
        })
    }

    /// A size that named a constant nobody gave: nothing computed from it is a number.
    fn complete(&self, range: Range) -> Option<Range> {
        (!self.unresolved.get()).then_some(range)
    }

    fn resolved_struct<'s>(&self, s: &'s grammar::Struct) -> Resolved<'s, ()> {
        let fields = s
            .fields()
            .iter()
            .map(|f| Member {
                name: Cow::Owned(f.name()),
                ty: Cow::Owned(self.freeze(f.field_type())),
                optional: f.optional(),
            })
            .collect();
        let max_depth = s
            .annotations()
            .into_iter()
            .find(|a| a.key() == "max_depth")
            .and_then(|a| a.value())
            .and_then(|v| v.trim().parse().ok());
        Resolved {
            key: s as *const grammar::Struct as usize,
            scope: (),
            decl: Decl::Struct { name: Cow::Owned(s.name()), fields, max_depth },
        }
    }
}

impl<L: SizeLookup> Lookup for Flat<'_, L> {
    type Scope = ();

    fn resolve(&self, _: (), name: &str) -> Option<Resolved<'_, ()>> {
        let bare = name.rsplit("::").next().unwrap_or(name);
        Some(match self.lookup.resolve(bare)? {
            SizeTarget::Struct(s) => self.resolved_struct(s),
            SizeTarget::Enum(e) => Resolved {
                key: e as *const grammar::Enum as usize,
                scope: (),
                decl: Decl::Enum {
                    name: Cow::Owned(e.name()),
                    variants: e.variants().iter().map(|v| Cow::Owned(v.name.text.clone())).collect(),
                },
            },
            // Aliases are transparent, and the grammar's has no `@max_depth`
            // we read.
            SizeTarget::Alias(target) => Resolved {
                key: target as *const Type as usize,
                scope: (),
                decl: Decl::Alias { target: Cow::Owned(self.freeze(target)), max_depth: None },
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::grammar::Declaration;
    use super::*;

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
                        Declaration::TypeAlias(t) if t.name() == bare_name => {
                            return Some(SizeTarget::Alias(t.target_type()));
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

    fn packed() -> &'static Sizes {
        rules("packed@1").unwrap()
    }

    fn msgpack() -> &'static Sizes {
        rules("msgpack@1").unwrap()
    }

    fn named_struct<'a>(doc: &'a grammar::Document, name: &str) -> &'a grammar::Struct {
        for decl in &doc.0 {
            if let Declaration::Struct(s) = &**decl {
                if s.name() == name {
                    return s;
                }
            }
        }
        panic!("no struct named {name}");
    }

    fn field_range(src: &str, rules: &Sizes) -> Option<Range> {
        let doc = parse(src);
        let lookup = TestLookup { docs: vec![&doc] };
        let s = named_struct(&doc, "S");
        range_of_field(&s.fields()[0], rules, &lookup, &|_| None)
    }

    fn range(min: u64, max: Option<u64>) -> Option<Range> {
        Some(Range { min, max, upper_bound: false })
    }

    /// Binds `vocabulary` and the sizer: a primitive's declared bit width is
    /// what `packed@1` writes for it, and one without (`string`) has no bound.
    #[test]
    fn primitive_bits_agree_with_packed() {
        for p in super::super::vocabulary::PRIMITIVES {
            let got = field_range(&format!("struct S {{ field: {} }}", p.name), packed());
            match p.bits {
                Some(bits) => {
                    let bytes = (bits / 8) as u64;
                    assert_eq!(got, range(bytes, Some(bytes)), "type {}", p.name)
                }
                // What isn't at one width isn't fixed, and a `string` has no bound.
                None => {
                    assert!(got.is_some_and(|r| !r.is_fixed()), "type {}", p.name);
                    if p.name == "string" {
                        assert_eq!(got.map(|r| r.max), Some(None));
                    }
                }
            }
        }
    }

    #[test]
    fn a_bound_makes_a_string_finite() {
        assert_eq!(field_range("struct S { field: string }", packed()), range(1, None));
        assert_eq!(field_range("struct S { field: char[max 3] }", packed()), range(1, Some(13)));
        assert_eq!(field_range("struct S { field: char[max 3] }", msgpack()), range(1, Some(13)));
        assert_eq!(field_range("struct S { field: u8[max 3] }", msgpack()), range(1, Some(7)));
        assert_eq!(field_range("struct S { field: u8[] }", packed()), range(1, None));
    }

    #[test]
    fn unit_is_zero_bytes_in_packed() {
        assert_eq!(field_range("struct S { field: () }", packed()), range(0, Some(0)));
    }

    #[test]
    fn a_fixed_array_is_its_length_times_the_element() {
        assert_eq!(field_range("struct S { field: u32[4] }", packed()), range(16, Some(16)));
        assert_eq!(field_range("struct S { field: string[2] }", packed()), range(2, None));
    }

    #[test]
    fn an_optional_field_is_absent_or_the_type() {
        // `packed@1`: a tag byte, then the value.
        assert_eq!(field_range("struct S { optional field: u32 }", packed()), range(1, Some(5)));
    }

    #[test]
    fn a_union_has_no_size() {
        assert_eq!(field_range("struct S { field: union(u8 u64) }", packed()), None);
    }

    #[test]
    fn a_nonsense_array_size_is_no_size_not_a_panic() {
        // `IntegerLiteral`'s grammar pattern is `-?\d+`, reused generically
        // for array sizes — a mid-edit buffer can contain this: a negative size
        // freezes as 0 and the type as an empty fixed array.
        assert!(field_range("struct S { field: u32[-1] }", packed()).is_some());
        // Too many bytes for anything to write: not sized.
        assert_eq!(field_range("struct S { field: u8[99999999999] }", msgpack()), None);
    }

    #[test]
    fn an_unresolved_name_or_constant_is_no_size() {
        assert_eq!(field_range("struct S { field: Missing }", packed()), None);
        assert_eq!(field_range("struct S { field: char[max MAX_NAME] }", packed()), None);
    }

    #[test]
    fn a_constant_gives_its_value() {
        let doc = parse("struct S { field: char[max MAX_NAME] }");
        let lookup = TestLookup { docs: vec![&doc] };
        let field = &named_struct(&doc, "S").fields()[0];
        let got = range_of_field(field, packed(), &lookup, &|name| (name == "MAX_NAME").then_some(3));
        assert_eq!(got, range(1, Some(13)));
    }

    #[test]
    fn nested_structs_add_up() {
        let doc = parse("struct Inner { a: u32 b: u8 } struct Outer { inner: Inner }");
        let lookup = TestLookup { docs: vec![&doc] };
        assert_eq!(range_of_struct(named_struct(&doc, "Outer"), packed(), &lookup, &|_| None), range(5, Some(5)));
        assert_eq!(range_of_struct(named_struct(&doc, "Outer"), msgpack(), &lookup, &|_| None), range(4, Some(9)));
    }

    #[test]
    fn a_struct_with_a_string_field_has_no_maximum() {
        let doc = parse("struct S { a: u32 b: string }");
        let lookup = TestLookup { docs: vec![&doc] };
        assert_eq!(range_of_struct(named_struct(&doc, "S"), packed(), &lookup, &|_| None), range(5, None));
    }

    #[test]
    fn an_enum_is_its_discriminant() {
        let doc = parse("enum E { A B C }");
        for decl in &doc.0 {
            if let Declaration::Enum(e) = &**decl {
                assert_eq!(range_of_enum(e, packed()), range(1, Some(1)));
                // `msgpack@1` writes the name: a string of one byte.
                assert_eq!(range_of_enum(e, msgpack()), range(2, Some(2)));
            }
        }
    }

    #[test]
    fn an_enum_widens_past_128_variants() {
        let variants: String = (0..300).map(|i| format!("V{i} ")).collect();
        let doc = parse(&format!("enum E {{ {variants} }}"));
        for decl in &doc.0 {
            if let Declaration::Enum(e) = &**decl {
                // A varint: one byte below 128, two above.
                assert_eq!(range_of_enum(e, packed()), range(1, Some(2)));
            }
        }
    }

    #[test]
    fn a_struct_containing_itself_without_a_limit_is_unbounded() {
        let doc = parse("struct Node { optional next: Node }");
        let lookup = TestLookup { docs: vec![&doc] };
        assert_eq!(range_of_struct(named_struct(&doc, "Node"), packed(), &lookup, &|_| None), range(1, None));
    }

    #[test]
    fn a_struct_containing_itself_under_a_limit_is_finite() {
        let doc = parse("@max_depth = 3\nstruct Node { optional next: Node }");
        let lookup = TestLookup { docs: vec![&doc] };
        assert_eq!(range_of_struct(named_struct(&doc, "Node"), packed(), &lookup, &|_| None), range(1, Some(3)));
    }

    #[test]
    fn a_required_self_reference_has_no_size_not_a_hang() {
        // This is precisely the case `ir::validation::validator`'s
        // `detect_cycle` currently mishandles: frozen IR has already
        // dropped the `[4]` literal by the time that check runs, so a
        // fixed-size self-reference slips past its `ends_with("[]")`
        // cycle-escape. The sizer must not trust that check, and must not
        // hang on it: a value that has to hold itself has no end.
        let doc = parse("struct Node { next: Node[4] }");
        let lookup = TestLookup { docs: vec![&doc] };
        assert_eq!(range_of_struct(named_struct(&doc, "Node"), packed(), &lookup, &|_| None), None);
    }

    #[test]
    fn a_mutual_cycle_has_no_size() {
        let doc = parse("struct A { b: B } struct B { a: A }");
        let lookup = TestLookup { docs: vec![&doc] };
        assert_eq!(range_of_struct(named_struct(&doc, "A"), packed(), &lookup, &|_| None), None);
    }

    #[test]
    fn a_cycle_across_files_has_no_size_not_a_hang() {
        // `A` (file 1) refers to `B` (file 2), which refers back to `A` —
        // nothing in the codebase guards this shape today (the validator's
        // own cycle check is scoped to one file's units).
        let file1 = parse("struct A { b: B }");
        let file2 = parse("struct B { a: A }");
        let lookup = TestLookup { docs: vec![&file1, &file2] };
        assert_eq!(range_of_struct(named_struct(&file1, "A"), packed(), &lookup, &|_| None), None);
    }
}
