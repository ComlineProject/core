//! The authority on which `settings` keys exist, what each accepts, and
//! how a key resolves against the scoping layers.
//!
//! Before this module the answer lived in three `format!` expressions
//! (`enforcement`'s two candidate arrays and `severity`'s one key), with
//! [`super::catalog`] a hand-maintained mirror of them for the editor -
//! whose own doc admitted it had to be updated by hand whenever those
//! arrays changed. Nothing enumerated the keys, so an unrecognised one was
//! silently accepted and the editor guessed at value types. Everything now
//! derives from [`SHAPES`]: candidate generation, key matching, the
//! built-in defaults, the editor catalog, and the two freeze-time
//! validation passes.
//!
//! # Why two shape variants rather than one pattern list
//!
//! Every key but one has the fixed arity
//! `[<decl-path>.]<group>[.<name>].<leaf>`, which [`PathShape`] describes.
//! `mode` does not: it is legal in *any* dictionary at *any* depth
//! (`struct.annotations.framing.mode` as readily as a bare `mode`).
//! Expressing that as a wildcard segment in a uniform pattern list would
//! need a backtracking matcher and would forfeit the exact-arity
//! disjointness that makes [`match_path`] unambiguous (see
//! `all_shapes_are_pairwise_disjoint`). It is also consumed differently -
//! [`super::merge`] reads it structurally while recursing dictionaries and
//! never looks it up by path - so candidate-key generation is meaningless
//! for it. It stays in [`SHAPES`] so defaults, docs, completion and
//! validation still enumerate it uniformly: one registry, one explicit arm.

use crate::config_value::{StrVariant, ValueType};
use crate::diagnostics::DiagnosticCode;
use crate::schema::idl::annotations::AnnotationScope;

/// The closed set of declaration sites a key may be scoped to - exactly
/// the six prefixes `enforcement` previously passed as bare string
/// literals at each of its call sites.
///
/// Making this closed is what dissolves the reverse-parsing ambiguity
/// [`super::catalog`] used to document as unfixable: a decl path only ever
/// occupies position 0, where no [`PathShape::group`] can appear, so the
/// two vocabularies never overlap and a written key has exactly one parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclPath {
    Struct,
    StructField,
    Error,
    ErrorField,
    Protocol,
    ProtocolFunction,
}

impl DeclPath {
    /// Longest-first, which [`DeclPath::strip`] depends on: `struct.field`
    /// must be tried before `struct`, or the two-segment paths would never
    /// match.
    pub const ALL: [DeclPath; 6] = [
        DeclPath::StructField,
        DeclPath::ErrorField,
        DeclPath::ProtocolFunction,
        DeclPath::Struct,
        DeclPath::Error,
        DeclPath::Protocol,
    ];

    pub const fn segments(self) -> &'static [&'static str] {
        match self {
            DeclPath::Struct => &["struct"],
            DeclPath::StructField => &["struct", "field"],
            DeclPath::Error => &["error"],
            DeclPath::ErrorField => &["error", "field"],
            DeclPath::Protocol => &["protocol"],
            DeclPath::ProtocolFunction => &["protocol", "function"],
        }
    }

    /// The dotted spelling used inside a key. Byte-identical to the
    /// literals `enforcement` used to pass, so every pinned diagnostic
    /// message is unchanged.
    pub const fn dotted(self) -> &'static str {
        match self {
            DeclPath::Struct => "struct",
            DeclPath::StructField => "struct.field",
            DeclPath::Error => "error",
            DeclPath::ErrorField => "error.field",
            DeclPath::Protocol => "protocol",
            DeclPath::ProtocolFunction => "protocol.function",
        }
    }

    /// Which annotation-attachment scope this declaration site is. Folds
    /// what used to be a third, independent vocabulary for one concept
    /// ([`AnnotationScope`]) onto this finer one, so the editor can offer
    /// decl-scoped settings keys for the site the cursor is actually in.
    pub const fn annotation_scope(self) -> AnnotationScope {
        match self {
            DeclPath::Struct | DeclPath::Error | DeclPath::Protocol => AnnotationScope::Leading,
            DeclPath::StructField | DeclPath::ErrorField => AnnotationScope::Field,
            DeclPath::ProtocolFunction => AnnotationScope::Function,
        }
    }

    /// Split a leading decl path off `segments`, longest-first. Returns
    /// what remains after it. Requires something to remain, so a key that
    /// is *only* a decl path is not treated as scoped - use
    /// [`DeclPath::split_prefix`] when an exact match is also acceptable.
    ///
    /// Two lifetimes on purpose: the returned sub-slice borrows the slice
    /// (`'s`), while the segment strings it holds borrow their own source
    /// (`'a`) - so a caller can match against a temporary `Vec` of
    /// segments and still keep the matched name.
    pub fn strip<'s, 'a>(segments: &'s [&'a str]) -> (Option<DeclPath>, &'s [&'a str]) {
        for decl in DeclPath::ALL {
            let p = decl.segments();
            if segments.len() > p.len() && segments.starts_with(p) {
                return (Some(decl), &segments[p.len()..]);
            }
        }
        (None, segments)
    }

    /// Like [`DeclPath::strip`] but also matches when `segments` *is*
    /// exactly a decl path, leaving an empty remainder - needed when
    /// validating that a dictionary position exists (`struct.mode`).
    pub fn split_prefix<'s, 'a>(segments: &'s [&'a str]) -> (Option<DeclPath>, &'s [&'a str]) {
        for decl in DeclPath::ALL {
            let p = decl.segments();
            if segments.starts_with(p) {
                return (Some(decl), &segments[p.len()..]);
            }
        }
        (None, segments)
    }
}

/// What fills a shape's one user-chosen segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameKind {
    /// An identifier-shaped annotation or validator name.
    Name,
    /// A diagnostic code id - contains `::` and `-`, so in `.ids` it must
    /// be written as a quoted key segment.
    CodeId,
}

impl NameKind {
    /// The placeholder spelling used in a displayed key pattern. The
    /// editor splits patterns on `.` and compares segments against these,
    /// so they are load-bearing, not decorative.
    pub const fn placeholder(self) -> &'static str {
        match self {
            NameKind::Name => "<name>",
            NameKind::CodeId => "<code>",
        }
    }
}

/// Whether a key may be scoped to a declaration site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scoping {
    /// Decl-scoped key first, then the unscoped global form.
    DeclScopedThenGlobal,
    /// Exact shape only, never decl-prefixed. `diagnostics.<code>.level`
    /// is resolved package- and schema-wide only, so advertising a scoped
    /// form would promise something that silently does nothing.
    GlobalOnly,
}

/// Prose for one shape. Carries both the named and the name-omitted
/// wording, plus per-name notes - replacing what used to be a hardcoded
/// `if name == "framing"` branch in the catalog and two separately
/// maintained, already-diverged copies of every description.
#[derive(Debug, PartialEq, Eq)]
pub struct ShapeDoc {
    pub summary: &'static str,
    pub description: &'static str,
    /// Wording for the name-omitted form. `None` exactly when
    /// [`PathShape::coarse`] is `false`.
    pub coarse_summary: Option<&'static str>,
    pub coarse_description: Option<&'static str>,
    /// Extra sentence appended for one specific name, keyed by that name.
    pub name_notes: &'static [(&'static str, &'static str)],
}

/// A fixed-arity key: `[<decl-path>.]<group>[.<name>].<leaf>`.
#[derive(Debug, PartialEq, Eq)]
pub struct PathShape {
    /// Stable identity for lookups, independent of display spelling.
    pub id: &'static str,
    pub group: &'static str,
    pub leaf: &'static str,
    pub name: NameKind,
    /// Whether the name segment may be omitted, giving a coarser
    /// "anything in this group" form.
    ///
    /// This single bool encodes a real, deliberate asymmetry: validators
    /// have a bare `validators.allowed` fallback, annotations have none.
    /// It used to be implicit in two divergent hand-written candidate
    /// arrays.
    pub coarse: bool,
    pub scoping: Scoping,
    pub value: ValueType,
    /// The code to report a value of the wrong type under. Lives on the
    /// shape rather than in whichever pass happens to notice, so every
    /// reporter agrees - `conformance` catches these at freeze time and
    /// `severity` catches the same key's value again when it resolves an
    /// override, and the two must not invent different codes for one
    /// mistake.
    pub malformed: DiagnosticCode,
    pub doc: ShapeDoc,
}

/// A reserved bare word, legal as a key in any dictionary at any depth.
#[derive(Debug, PartialEq, Eq)]
pub struct ReservedShape {
    pub id: &'static str,
    pub word: &'static str,
    pub value: ValueType,
    pub malformed: DiagnosticCode,
    pub doc: ShapeDoc,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Shape {
    Path(PathShape),
    Reserved(ReservedShape),
}

impl Shape {
    pub fn id(&self) -> &'static str {
        match self {
            Shape::Path(p) => p.id,
            Shape::Reserved(r) => r.id,
        }
    }

    pub fn value(&self) -> &ValueType {
        match self {
            Shape::Path(p) => &p.value,
            Shape::Reserved(r) => &r.value,
        }
    }

    pub fn malformed(&self) -> DiagnosticCode {
        match self {
            Shape::Path(p) => p.malformed,
            Shape::Reserved(r) => r.malformed,
        }
    }
}

// Shape ids. Used instead of bare strings so a typo is a compile error.
pub const MODE: &str = "mode";
pub const ANNOTATIONS_ALLOWED: &str = "annotations.allowed";
pub const VALIDATORS_ALLOWED: &str = "validators.allowed";
pub const DIAGNOSTICS_LEVEL: &str = "diagnostics.level";

/// The only value `mode` accepts.
pub const MODE_REPLACE: &str = "replace";

/// Every severity a diagnostic override may name. `allow` drops the
/// diagnostic entirely; the other four map onto
/// [`crate::diagnostics::Severity`]. Note `warn` deliberately differs from
/// the enum's own `Warning` spelling, so this is a real table rather than
/// a derived one.
const LEVELS: &[StrVariant] = &[
    StrVariant::new("allow"),
    StrVariant::new("hint"),
    StrVariant::new("info"),
    StrVariant::new("warn"),
    StrVariant::new("error"),
];

/// Every recognised settings-key shape.
///
/// Order is load-bearing: [`listing`] derives from it, and the language
/// server's completion tests pin the order keys are offered in.
pub const SHAPES: &[Shape] = &[
    Shape::Reserved(ReservedShape {
        id: MODE,
        word: "mode",
        value: ValueType::Keyword(&[MODE_REPLACE]),
        malformed: DiagnosticCode::error("settings::invalid-mode"),
        doc: ShapeDoc {
            summary: "merge behavior for this subtree",
            description: "Reserved key. Its value must be the bare keyword `replace` - \
                when present, this subtree replaces the corresponding subtree from the \
                layer below instead of merging into it. Absent (the default) means merge.",
            coarse_summary: None,
            coarse_description: None,
            name_notes: &[],
        },
    }),
    Shape::Path(PathShape {
        id: ANNOTATIONS_ALLOWED,
        group: "annotations",
        leaf: "allowed",
        name: NameKind::Name,
        // No coarse form: nothing ever resolves a bare
        // `annotations.allowed`, and offering one would promise a policy
        // that is never consulted.
        coarse: false,
        scoping: Scoping::DeclScopedThenGlobal,
        value: ValueType::Bool,
        malformed: DiagnosticCode::error("settings::malformed-value"),
        doc: ShapeDoc {
            summary: "whether this annotation is permitted here",
            description: "Whether the named annotation is permitted on declarations this key's \
                scope covers. `true` allows it, `false` forbids it (a hard build error naming \
                the annotation and this key). No key at any level means allowed by default.",
            coarse_summary: None,
            coarse_description: None,
            name_notes: &[(
                "framing",
                "Comline's built-in default already forbids `framing` specifically - with no \
                 key written anywhere it is forbidden, unlike every other annotation.",
            )],
        },
    }),
    Shape::Path(PathShape {
        id: VALIDATORS_ALLOWED,
        group: "validators",
        leaf: "allowed",
        name: NameKind::Name,
        coarse: true,
        scoping: Scoping::DeclScopedThenGlobal,
        value: ValueType::Bool,
        malformed: DiagnosticCode::error("settings::malformed-value"),
        doc: ShapeDoc {
            summary: "whether this validator is permitted here",
            description: "Whether the named validator is permitted on declarations this \
                key's scope covers. `true` allows it, `false` forbids it (a hard build \
                error naming the validator and this key). No key at any level means \
                allowed by default.",
            coarse_summary: Some("whether any validator is permitted here"),
            coarse_description: Some(
                "Whether validators in general are permitted on declarations this key's \
                 scope covers, when no more specific `validators.<name>.allowed` key \
                 matches first. `true` allows, `false` forbids (a hard build error naming \
                 the validator and this key).",
            ),
            name_notes: &[],
        },
    }),
    Shape::Path(PathShape {
        id: DIAGNOSTICS_LEVEL,
        group: "diagnostics",
        leaf: "level",
        name: NameKind::CodeId,
        coarse: false,
        scoping: Scoping::GlobalOnly,
        value: ValueType::StrEnum(LEVELS),
        malformed: DiagnosticCode::error("settings::invalid-diagnostic-level"),
        doc: ShapeDoc {
            summary: "overrides a diagnostic code's effective severity",
            description: "Overrides the severity of the named diagnostic code project-wide \
                (package- and schema-level `settings` blocks only - not decl-scoped). \
                `allow` suppresses it entirely. Only a code whose own intrinsic severity is \
                non-Error can be overridden this way - an Error-severity code's level is \
                fixed and this key has no effect on it.",
            coarse_summary: None,
            coarse_description: None,
            name_notes: &[],
        },
    }),
];

/// Look up a path shape by its id. Panics if the id isn't in [`SHAPES`] -
/// callers pass one of this module's own consts, and
/// `every_shape_const_resolves` guards that.
pub fn path_shape(id: &str) -> &'static PathShape {
    SHAPES
        .iter()
        .find_map(|s| match s {
            Shape::Path(p) if p.id == id => Some(p),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no path shape registered with id '{id}'"))
}

/// The shape governing `@annotation` usage.
pub fn annotations_allowed() -> &'static PathShape {
    path_shape(ANNOTATIONS_ALLOWED)
}

/// The shape governing `@validators` entries.
pub fn validators_allowed() -> &'static PathShape {
    path_shape(VALIDATORS_ALLOWED)
}

/// The shape governing diagnostic severity overrides.
pub fn diagnostics_level() -> &'static PathShape {
    path_shape(DIAGNOSTICS_LEVEL)
}

/// The reserved `mode` shape.
pub fn mode() -> &'static ReservedShape {
    SHAPES
        .iter()
        .find_map(|s| match s {
            Shape::Reserved(r) if r.id == MODE => Some(r),
            _ => None,
        })
        .expect("the `mode` shape must be registered")
}

impl PathShape {
    /// The concrete key for one (scope, name) combination.
    pub fn key(&self, decl: Option<DeclPath>, name: Option<&str>) -> String {
        let mut s = String::new();
        if let Some(d) = decl {
            s.push_str(d.dotted());
            s.push('.');
        }
        s.push_str(self.group);
        if let Some(n) = name {
            s.push('.');
            s.push_str(n);
        }
        s.push('.');
        s.push_str(self.leaf);
        s
    }

    /// The ordered candidate list a resolver walks, most specific first.
    ///
    /// Scope is the outer, dominant axis and name-specificity the inner
    /// tie-break - so a decl-scoped coarse key beats a global named one.
    /// Both of `enforcement`'s previously hand-written arrays fall out of
    /// this one function plus [`PathShape::coarse`].
    pub fn candidates(&self, decl: DeclPath, name: &str) -> Vec<String> {
        let scopes = [Some(decl), None];
        let start = usize::from(self.scoping == Scoping::GlobalOnly);
        let mut out = Vec::with_capacity(4);
        for scope in &scopes[start..] {
            out.push(self.key(*scope, Some(name)));
            if self.coarse {
                out.push(self.key(*scope, None));
            }
        }
        out
    }

    /// The displayed pattern, with the name segment as a placeholder.
    pub fn pattern(&self, coarse: bool) -> String {
        if coarse {
            format!("{}.{}", self.group, self.leaf)
        } else {
            format!("{}.{}.{}", self.group, self.name.placeholder(), self.leaf)
        }
    }
}

/// A written key, matched against the registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Match<'a> {
    pub shape: &'static Shape,
    /// The declaration site this key is scoped to, if any.
    pub decl: Option<DeclPath>,
    /// The user-chosen name or code id. `None` for the coarse form and for
    /// a reserved word.
    pub name: Option<&'a str>,
}

impl<'a> Match<'a> {
    /// Whether this matched the name-omitted form of its shape.
    pub fn is_coarse(&self) -> bool {
        matches!(self.shape, Shape::Path(_)) && self.name.is_none()
    }

    /// The code a wrongly-typed value here is reported under.
    pub fn malformed_code(&self) -> DiagnosticCode {
        self.shape.malformed()
    }

    /// What this key's value must be.
    pub fn value_type(&self) -> &'static ValueType {
        match self.shape {
            Shape::Path(p) => &p.value,
            Shape::Reserved(r) => &r.value,
        }
    }
}

/// Match a written key's segments against the registry, prefix-anchored.
///
/// Replaces suffix matching, which accepted `nonsense.validators.allowed`
/// (silently inert) and could not tell a validator *named* `validators`
/// from the coarse form. Anchoring at the front against a closed
/// [`DeclPath`] set plus exact-arity shapes makes the parse unique - see
/// `all_shapes_are_pairwise_disjoint`.
pub fn match_path<'a>(segments: &[&'a str]) -> Option<Match<'a>> {
    if segments.is_empty() {
        return None;
    }

    // Reserved words first: `mode` is legal at any depth, so it is
    // identified by its last segment plus a valid dictionary position,
    // not by arity.
    let reserved = SHAPES.iter().find_map(|s| match s {
        Shape::Reserved(r) if r.word == segments[segments.len() - 1] => Some((s, r)),
        _ => None,
    });
    if let Some((shape, _)) = reserved {
        let prefix = &segments[..segments.len() - 1];
        return is_group_position(prefix).then_some(Match { shape, decl: None, name: None });
    }

    match_path_shape(segments)
}

/// The fixed-arity half of [`match_path`], without the reserved-word
/// check. Split out so [`is_group_position`] can ask "is this already a
/// complete key?" without recursing back through the reserved-word arm.
fn match_path_shape<'a>(segments: &[&'a str]) -> Option<Match<'a>> {
    for (decl, rest) in prefix_splits(segments) {
        for shape in SHAPES {
            let Shape::Path(p) = shape else { continue };
            if decl.is_some() && p.scoping == Scoping::GlobalOnly {
                continue;
            }
            match rest {
                [group, name, leaf] if *group == p.group && *leaf == p.leaf => {
                    return Some(Match { shape, decl, name: Some(name) });
                }
                [group, leaf] if p.coarse && *group == p.group && *leaf == p.leaf => {
                    return Some(Match { shape, decl, name: None });
                }
                _ => {}
            }
        }
    }

    None
}

/// The ways `segments` may be read: unscoped, and - when it starts with
/// one - with a decl path stripped. Only one can ever match a shape, since
/// a decl path and a group name never share position 0.
fn prefix_splits<'s, 'a>(segments: &'s [&'a str]) -> Vec<(Option<DeclPath>, &'s [&'a str])> {
    let mut out = vec![(None, segments)];
    let (decl, rest) = DeclPath::strip(segments);
    if let Some(d) = decl {
        out.push((Some(d), rest));
    }
    out
}

/// Whether a key is one the registry recognises at all.
pub fn is_known(segments: &[&str]) -> bool {
    match_path(segments).is_some()
}

/// Whether a dictionary can legally exist at this path - i.e. whether a
/// reserved word like `mode` written inside it would mean anything.
///
/// Without this, `mode`'s any-depth nature would be a hole wide enough to
/// swallow most typos: `annotations.framng.mode` would validate purely
/// because it ends in `mode`.
pub fn is_group_position(segments: &[&str]) -> bool {
    // A complete key is a leaf value - no dictionary lives there, so
    // nothing may be nested under it. This is what keeps
    // `validators.allowed.mode` out: `validators.allowed` is already a
    // whole key (the coarse form), not a group whose members include
    // `mode`.
    if match_path_shape(segments).is_some() {
        return false;
    }

    let (decl, rest) = DeclPath::split_prefix(segments);

    // The root, or a bare decl path - both real dictionary positions.
    if rest.is_empty() {
        return true;
    }

    SHAPES.iter().any(|shape| {
        let Shape::Path(p) = shape else { return false };
        if decl.is_some() && p.scoping == Scoping::GlobalOnly {
            return false;
        }
        match rest {
            // `annotations`, or `struct.validators`
            [group] => *group == p.group,
            // `annotations.framing` - the name level, which may hold `mode`
            [group, _name] => *group == p.group,
            _ => false,
        }
    })
}

/// Whether `segments` enters a group this registry owns. Keys that do are
/// held to the registry's shapes; anything else is left alone, keeping
/// `settings` an open namespace at the top level for forward
/// compatibility.
pub fn enters_known_group(segments: &[&str]) -> bool {
    let (_, rest) = DeclPath::split_prefix(segments);
    let Some(first) = rest.first() else {
        return false;
    };
    SHAPES.iter().any(|shape| match shape {
        Shape::Path(p) => p.group == *first,
        Shape::Reserved(r) => r.word == *first,
    }) || SHAPES.iter().any(|shape| match shape {
        // A trailing reserved word anywhere inside a recognised position.
        Shape::Reserved(r) => segments.last() == Some(&r.word),
        _ => false,
    })
}

/// One displayable key pattern. `key_pattern` is a `String` because it is
/// assembled from [`SHAPES`] rather than written out by hand.
pub struct KeyDoc {
    pub key_pattern: String,
    pub summary: &'static str,
    pub description: &'static str,
    /// What this key's value must be, so the editor can offer real values
    /// rather than assuming every key takes a boolean.
    pub value: ValueType,
}

/// Every recognised shape as a flat listing, for "what's configurable
/// here". Both forms of a shape with a coarse variant are listed, named
/// first - the order the editor's own tests pin.
pub fn listing() -> Vec<KeyDoc> {
    let mut out = Vec::new();
    for shape in SHAPES {
        match shape {
            Shape::Reserved(r) => out.push(KeyDoc {
                key_pattern: r.word.to_string(),
                summary: r.doc.summary,
                description: r.doc.description,
                value: r.value,
            }),
            Shape::Path(p) => {
                out.push(KeyDoc {
                    key_pattern: p.pattern(false),
                    summary: p.doc.summary,
                    description: p.doc.description,
                    value: p.value,
                });
                if p.coarse {
                    out.push(KeyDoc {
                        key_pattern: p.pattern(true),
                        summary: p.doc.coarse_summary.unwrap_or(p.doc.summary),
                        description: p.doc.coarse_description.unwrap_or(p.doc.description),
                        value: p.value,
                    });
                }
            }
        }
    }
    out
}

/// The full prose for a matched key, including any per-name note and the
/// generated description of what its value must be.
pub fn doc_for(m: &Match) -> KeyDoc {
    let (pattern, summary, description) = match m.shape {
        Shape::Reserved(r) => (r.word.to_string(), r.doc.summary, r.doc.description),
        Shape::Path(p) => {
            if m.is_coarse() {
                (
                    p.pattern(true),
                    p.doc.coarse_summary.unwrap_or(p.doc.summary),
                    p.doc.coarse_description.unwrap_or(p.doc.description),
                )
            } else {
                (p.pattern(false), p.doc.summary, p.doc.description)
            }
        }
    };

    let mut description = description.to_string();
    if let (Shape::Path(p), Some(name)) = (m.shape, m.name) {
        if let Some((_, note)) = p.doc.name_notes.iter().find(|(n, _)| *n == name) {
            description.push(' ');
            description.push_str(note);
        }
    }

    KeyDoc {
        key_pattern: pattern,
        summary,
        // Leaked so `KeyDoc`'s fields stay uniform for the editor, which
        // holds these for the lifetime of a hover. One small leak per
        // hover of a named key with a note is acceptable; the alternative
        // is making every description an owned `String` and churning both
        // this module's callers and the editor's.
        description: Box::leak(description.into_boxed_str()),
        value: *m.value_type(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segs(key: &str) -> Vec<&str> {
        key.split('.').collect()
    }

    #[test]
    fn every_shape_const_resolves() {
        // Guards the `panic!` in `path_shape`.
        assert_eq!(annotations_allowed().id, ANNOTATIONS_ALLOWED);
        assert_eq!(validators_allowed().id, VALIDATORS_ALLOWED);
        assert_eq!(diagnostics_level().id, DIAGNOSTICS_LEVEL);
        assert_eq!(mode().id, MODE);
    }

    #[test]
    fn coarse_prose_is_present_exactly_when_a_coarse_form_exists() {
        for shape in SHAPES {
            if let Shape::Path(p) = shape {
                assert_eq!(
                    p.coarse,
                    p.doc.coarse_summary.is_some(),
                    "{} coarse flag and coarse prose disagree",
                    p.id
                );
                assert_eq!(p.coarse, p.doc.coarse_description.is_some(), "{}", p.id);
            }
        }
    }

    // ---- candidate generation reproduces the previous hand-written lists ----

    #[test]
    fn annotation_candidates_match_the_previous_hardcoded_pair() {
        let got = annotations_allowed().candidates(DeclPath::StructField, "framing");
        assert_eq!(
            got,
            vec![
                "struct.field.annotations.framing.allowed",
                "annotations.framing.allowed",
            ]
        );
    }

    #[test]
    fn validator_candidates_match_the_previous_hardcoded_four() {
        let got = validators_allowed().candidates(DeclPath::StructField, "StringBounds");
        assert_eq!(
            got,
            vec![
                "struct.field.validators.StringBounds.allowed",
                "struct.field.validators.allowed",
                "validators.StringBounds.allowed",
                "validators.allowed",
            ]
        );
    }

    #[test]
    fn decl_scoped_coarse_outranks_a_global_named_key() {
        // The documented tie-break, as an ordering property rather than a
        // comment: scope dominates, name-specificity breaks ties within it.
        let got = validators_allowed().candidates(DeclPath::Protocol, "Bar");
        let coarse_scoped = got.iter().position(|k| k == "protocol.validators.allowed");
        let named_global = got.iter().position(|k| k == "validators.Bar.allowed");
        assert!(coarse_scoped < named_global);
    }

    #[test]
    fn a_global_only_shape_never_generates_a_scoped_candidate() {
        let got = diagnostics_level().candidates(DeclPath::Struct, "validation::unknown-type");
        assert_eq!(got, vec!["diagnostics.validation::unknown-type.level"]);
    }

    // ---- matching ----

    #[test]
    fn matches_a_global_named_key() {
        let m = match_path(&segs("annotations.framing.allowed")).unwrap();
        assert_eq!(m.shape.id(), ANNOTATIONS_ALLOWED);
        assert_eq!(m.decl, None);
        assert_eq!(m.name, Some("framing"));
    }

    #[test]
    fn matches_a_decl_scoped_key_and_reports_its_scope() {
        let m = match_path(&segs("struct.field.validators.Bar.allowed")).unwrap();
        assert_eq!(m.shape.id(), VALIDATORS_ALLOWED);
        assert_eq!(m.decl, Some(DeclPath::StructField));
        assert_eq!(m.name, Some("Bar"));
    }

    #[test]
    fn matches_the_coarse_form_at_both_scopes() {
        let global = match_path(&segs("validators.allowed")).unwrap();
        assert!(global.is_coarse());
        assert_eq!(global.decl, None);

        let scoped = match_path(&segs("protocol.function.validators.allowed")).unwrap();
        assert!(scoped.is_coarse());
        assert_eq!(scoped.decl, Some(DeclPath::ProtocolFunction));
    }

    #[test]
    fn a_validator_literally_named_validators_is_no_longer_ambiguous() {
        // The case the old suffix matcher documented as unfixable: it
        // answered "coarse form", which was wrong. Exact arity settles it.
        let m = match_path(&segs("validators.validators.allowed")).unwrap();
        assert_eq!(m.name, Some("validators"));
        assert!(!m.is_coarse());
    }

    #[test]
    fn a_name_spelled_like_a_decl_path_still_matches_as_a_name() {
        let m = match_path(&segs("annotations.struct.allowed")).unwrap();
        assert_eq!(m.name, Some("struct"));
        assert_eq!(m.decl, None);
    }

    #[test]
    fn annotations_have_no_coarse_form() {
        // Deliberate asymmetry with validators, now expressed as data.
        assert!(match_path(&segs("annotations.allowed")).is_none());
        assert!(match_path(&segs("struct.annotations.allowed")).is_none());
    }

    #[test]
    fn diagnostics_level_is_never_decl_scoped() {
        assert!(match_path(&segs("diagnostics.some::code.level")).is_some());
        assert!(match_path(&segs("struct.diagnostics.some::code.level")).is_none());
    }

    #[test]
    fn a_garbage_prefix_no_longer_passes_as_a_scoped_key() {
        // Suffix matching accepted this and it was silently inert.
        assert!(match_path(&segs("nonsense.validators.allowed")).is_none());
    }

    #[test]
    fn a_misspelled_leaf_does_not_match() {
        assert!(match_path(&segs("annotations.framing.alowed")).is_none());
        assert!(match_path(&segs("validators.alowed")).is_none());
        assert!(match_path(&segs("diagnostics.some::code.levl")).is_none());
    }

    #[test]
    fn empty_input_matches_nothing() {
        assert!(match_path(&[]).is_none());
    }

    // ---- `mode` ----

    #[test]
    fn mode_matches_at_the_root_and_in_any_real_group_position() {
        for key in [
            "mode",
            "struct.mode",
            "struct.field.mode",
            "validators.mode",
            "annotations.mode",
            "annotations.framing.mode",
            "struct.field.validators.StringBounds.mode",
            "diagnostics.mode",
        ] {
            assert!(match_path(&segs(key)).is_some(), "{key} should match");
        }
    }

    #[test]
    fn mode_is_rejected_where_no_dictionary_can_exist() {
        // The hole this rule closes: without a position check, anything
        // ending in `mode` would validate.
        for key in [
            "nonsense.mode",
            "annotations.framing.allowed.mode",
            "validators.allowed.mode",
        ] {
            assert!(match_path(&segs(key)).is_none(), "{key} should not match");
        }
    }

    #[test]
    fn mode_accepts_only_the_bare_replace_keyword() {
        use crate::config_value::{check, ValueForm};
        let ty = mode().value;
        assert!(check(&ty, ValueForm::Name(MODE_REPLACE)).is_ok());
        assert!(check(&ty, ValueForm::Str(MODE_REPLACE)).is_err());
        assert!(check(&ty, ValueForm::Name("merge")).is_err());
    }

    // ---- the property the unknown-key error rests on ----

    #[test]
    fn all_shapes_are_pairwise_disjoint() {
        // `settings::unknown-key` is an Error-severity code, and settings
        // cannot soften an Error - so a false positive would be an
        // unfixable build break. That safety rests on every written key
        // having at most one parse. Enumerate every shape's concrete forms
        // at every scope and assert no two shapes can claim one key.
        let names = ["x", "validators", "annotations", "struct", "field", "allowed", "mode"];
        let mut seen: std::collections::HashMap<String, &'static str> = Default::default();

        for shape in SHAPES {
            let Shape::Path(p) = shape else { continue };
            let scopes: Vec<Option<DeclPath>> = if p.scoping == Scoping::GlobalOnly {
                vec![None]
            } else {
                DeclPath::ALL.iter().map(|d| Some(*d)).chain([None]).collect()
            };
            for scope in scopes {
                for name in names {
                    let mut forms = vec![p.key(scope, Some(name))];
                    if p.coarse {
                        forms.push(p.key(scope, None));
                    }
                    for key in forms {
                        if let Some(other) = seen.insert(key.clone(), p.id) {
                            assert_eq!(
                                other, p.id,
                                "key '{key}' is claimed by both '{other}' and '{}'",
                                p.id
                            );
                        }
                    }
                }
            }
        }

        // And every generated key must actually match back to its own shape.
        for (key, id) in &seen {
            let m = match_path(&segs(key))
                .unwrap_or_else(|| panic!("generated key '{key}' does not match back"));
            assert_eq!(m.shape.id(), *id, "key '{key}' matched the wrong shape");
        }
    }

    #[test]
    fn enters_known_group_gates_the_unknown_key_error() {
        // Inside a group core owns - held to the registry's shapes.
        assert!(enters_known_group(&segs("annotations.framing.alowed")));
        assert!(enters_known_group(&segs("validators.alowed")));
        assert!(enters_known_group(&segs("struct.field.validators.x.allowed")));
        assert!(enters_known_group(&segs("diagnostics.c.level")));
        assert!(enters_known_group(&segs("mode")));
        assert!(enters_known_group(&segs("struct.mode")));

        // Outside it - `settings` stays an open namespace at the top level.
        assert!(!enters_known_group(&segs("forbid_indexing")));
        assert!(!enters_known_group(&segs("max_depth")));
        assert!(!enters_known_group(&segs("label")));
        assert!(!enters_known_group(&segs("some.custom.thing")));
    }

    // ---- listing / docs ----

    #[test]
    fn listing_covers_every_shape_and_both_coarse_forms() {
        let patterns: Vec<String> = listing().into_iter().map(|d| d.key_pattern).collect();
        assert_eq!(
            patterns,
            vec![
                "mode",
                "annotations.<name>.allowed",
                "validators.<name>.allowed",
                "validators.allowed",
                "diagnostics.<code>.level",
            ]
        );
    }

    #[test]
    fn every_listed_pattern_is_matchable_with_its_placeholders_filled() {
        for doc in listing() {
            let filled: Vec<&str> = doc
                .key_pattern
                .split('.')
                .map(|s| if s.starts_with('<') { "x" } else { s })
                .collect();
            assert!(
                match_path(&filled).is_some(),
                "listed pattern '{}' does not match back",
                doc.key_pattern
            );
        }
    }

    #[test]
    fn listing_carries_each_keys_real_value_type() {
        let docs = listing();
        let level = docs.iter().find(|d| d.key_pattern.contains("level")).unwrap();
        // The editor used to assume every settings key took a boolean.
        assert!(matches!(level.value, ValueType::StrEnum(_)));
        let allowed = docs.iter().find(|d| d.key_pattern == "validators.allowed").unwrap();
        assert!(matches!(allowed.value, ValueType::Bool));
    }

    #[test]
    fn doc_for_appends_the_framing_note_only_for_framing() {
        let framing = match_path(&segs("annotations.framing.allowed")).unwrap();
        assert!(doc_for(&framing).description.contains("built-in default"));

        let other = match_path(&segs("annotations.idempotent.allowed")).unwrap();
        assert!(!doc_for(&other).description.contains("built-in default"));
    }

    #[test]
    fn doc_for_distinguishes_the_coarse_form() {
        let coarse = match_path(&segs("validators.allowed")).unwrap();
        assert_eq!(doc_for(&coarse).summary, "whether any validator is permitted here");
        let named = match_path(&segs("validators.Bar.allowed")).unwrap();
        assert_eq!(doc_for(&named).summary, "whether this validator is permitted here");
    }

    #[test]
    fn decl_paths_map_onto_annotation_scopes() {
        assert_eq!(DeclPath::Struct.annotation_scope(), AnnotationScope::Leading);
        assert_eq!(DeclPath::StructField.annotation_scope(), AnnotationScope::Field);
        assert_eq!(
            DeclPath::ProtocolFunction.annotation_scope(),
            AnnotationScope::Function
        );
    }

    #[test]
    fn longer_decl_paths_strip_before_shorter_prefixes() {
        let segments = segs("struct.field.validators.allowed");
        let (decl, rest) = DeclPath::strip(&segments);
        assert_eq!(decl, Some(DeclPath::StructField));
        assert_eq!(rest, ["validators", "allowed"]);
    }
}
