//! Which `@unverified` markers cover which unbounded values (core#222).
//!
//! A value without a maximum is a *cause* of giving up `bounded_size`, found
//! by walking what a protocol reaches: its functions, their arguments and
//! return, and through the named types, their fields. A marker
//! acknowledges the causes reached *through the element it sits on*:
//!
//! - **the protocol** covers everything its functions reach (the form that
//!   always existed);
//! - **a function** its arguments, return and thrown errors, and what they
//!   reach;
//! - **an argument** or **a field** its own type, and what that reaches;
//! - **a struct, an error or a type alias** the type itself, wherever it's
//!   used in this package: whichever way it's reached, the walk passes
//!   through it.
//!
//! A marker never subtracts from the computed set. Every path from a protocol
//! to a cause is judged on its own: one that passes through no marker leaves
//! the cause uncovered, and the build refuses it, naming the path. Of the
//! markers a path passes through, the innermost is the one it is credited to
//! (and what is frozen, [`Acknowledgement`]).
//!
//! Which scopes a marker may sit at is the catalog's: a marker covers
//! `bounded_size` only where `Guarantee::entry().scopes` allows that scope,
//! which is checked by the validator.
//!
//! Chains across packages are not here: a type another package declares is a
//! cause in this one, and its own package's marker satisfies only that package.
//! A type another *schema of this package* declares is [`Foreign::Acknowledged`]
//! where its marker covers all it holds.

use std::collections::{HashMap, HashSet};

use comline_catalog::Scope;

use super::{
    first_unbounded, has_max_depth, imported_errors, local_names, named, self_containing_structs,
};
use super::{UNBOUNDED, UNBOUNDED_DEPTH};
use crate::diagnostics::{Diagnostic, DiagnosticCode};
use crate::schema::ir::frozen::types::TypeRef;
use crate::schema::ir::frozen::unit::{ConfigValue, FrozenUnit, Guarantee};
use crate::schema::ir::unverified::{self, Acknowledgement};

const NOTHING_GIVEN_UP: DiagnosticCode = DiagnosticCode::warning("unverified::nothing-given-up");

/// What a schema says of a name it uses that another schema declares.
pub enum Foreign {
    /// Not another schema's type, or bounded where it's declared.
    Bounded,
    /// It holds a value without a maximum, which only a marker at the use (or
    /// above it) can acknowledge.
    Unbounded,
    /// It holds one, and a schema of this package acknowledged all it holds
    /// with markers on the type and what's under it: these, at their paths.
    Acknowledged(Vec<Acknowledgement>),
}

/// What gives up `bounded_size` for one protocol, once judged.
#[derive(Debug, Default)]
pub struct Given {
    /// Where its causes were acknowledged, by the innermost marker on each
    /// path. Empty if some cause was covered only by a protocol marker that
    /// doesn't list `bounded_size`: the validator says that is missing.
    pub acknowledgements: Vec<Acknowledgement>,
}

/// The outcome of [`analyze`].
#[derive(Debug, Default)]
pub struct Outcome {
    pub errors: Vec<Diagnostic>,
    pub warnings: Vec<Diagnostic>,
    /// The protocols that give up bounded size: every one with a cause some
    /// marker covers.
    pub protocols: HashMap<String, Given>,
}

/// A marker that covers `bounded_size` where it sits.
struct Mark {
    scope: Scope,
    path: String,
    reason: String,
    span: Option<(usize, usize)>,
}

/// Stands in a cover for a protocol marker that doesn't list `bounded_size`:
/// it covers, as such a marker always did, and the validator reports it.
const LEGACY: usize = usize::MAX;

/// Why a position has no maximum.
enum Cause {
    Value(String),
    Foreign(String),
    Depth,
}

/// One position that may be without a maximum: an argument, a return, an error
/// a function throws, a field, an alias's target, a struct's depth.
struct Elem {
    at: String,
    /// What it reads as in a path.
    segment: String,
    /// How a marker would be written for it.
    marker_on: String,
    mark: Option<usize>,
    cause: Option<Cause>,
    /// The names it refers to.
    names: Vec<String>,
    span: Option<(usize, usize)>,
}

struct Decl {
    mark: Option<usize>,
    elems: Vec<usize>,
}

struct Func {
    mark: Option<usize>,
    elems: Vec<usize>,
}

struct Protocol {
    name: String,
    mark: Option<usize>,
    /// Marked, but not for `bounded_size`.
    legacy: bool,
    functions: Vec<Func>,
}

#[derive(Default)]
struct Model {
    marks: Vec<Mark>,
    elems: Vec<Elem>,
    decls: Vec<Decl>,
    index: HashMap<String, usize>,
    protocols: Vec<Protocol>,
    /// Elements by the declaration they belong to.
    owner: HashMap<usize, usize>,
}

/// The marker's reason if it covers `bounded_size` at `scope`: a plain
/// reason, or the keyed form's entry for it, and only where the catalog
/// gives the guarantee that scope.
fn covers(parameters: &[FrozenUnit], scope: Scope) -> Option<String> {
    if !Guarantee::BoundedSize.entry().allows_scope(scope) {
        return None;
    }
    match unverified::marker(parameters)? {
        ConfigValue::Str(reason) => Some(reason.clone()),
        ConfigValue::Dict(entries) => entries.iter().find_map(|(key, value)| match value {
            ConfigValue::Str(reason) if key == Guarantee::BoundedSize.name() => {
                Some(reason.clone())
            }
            _ => None,
        }),
        _ => None,
    }
}

impl Model {
    fn mark(
        &mut self,
        parameters: &[FrozenUnit],
        scope: Scope,
        path: String,
        span: Option<(usize, usize)>,
    ) -> Option<usize> {
        let reason = covers(parameters, scope)?;
        self.marks.push(Mark {
            scope,
            path,
            reason,
            span,
        });
        Some(self.marks.len() - 1)
    }

    #[allow(clippy::too_many_arguments)]
    fn elem(
        &mut self,
        is_foreign: &dyn Fn(&str) -> bool,
        at: String,
        segment: String,
        marker_on: String,
        mark: Option<usize>,
        ty: &TypeRef,
        span: Option<(usize, usize)>,
    ) -> usize {
        let cause = first_unbounded(ty, is_foreign).map(|unbounded| match unbounded {
            TypeRef::Named(name) => Cause::Foreign(name.clone()),
            other => Cause::Value(other.to_string()),
        });
        let mut names = HashSet::new();
        named(ty, &HashMap::new(), &mut names, 0);
        let mut names: Vec<String> = names.into_iter().map(str::to_string).collect();
        names.sort();
        self.elems.push(Elem {
            at,
            segment,
            marker_on,
            mark,
            cause,
            names,
            span,
        });
        self.elems.len() - 1
    }

    fn build(units: &[FrozenUnit], foreign: &dyn Fn(&str) -> Foreign) -> Model {
        let mut model = Model::default();
        let recursive = self_containing_structs(units);
        let local = local_names(units);
        let is_foreign =
            |name: &str| !local.contains(name) && !matches!(foreign(name), Foreign::Bounded);

        // Declarations first, in order, so an index exists for every name.
        for unit in units {
            let name = match unit {
                FrozenUnit::Struct { name, .. }
                | FrozenUnit::Error {
                    name,
                    imported_from: None,
                    ..
                }
                | FrozenUnit::TypeAlias { name, .. } => name,
                _ => continue,
            };
            model.index.insert(name.clone(), model.decls.len());
            model.decls.push(Decl {
                mark: None,
                elems: vec![],
            });
        }

        for unit in units {
            match unit {
                FrozenUnit::Struct {
                    name,
                    fields,
                    parameters,
                    span,
                    ..
                } => {
                    let decl = model.index[name];
                    let mark = model.mark(parameters, Scope::Type, name.clone(), Some(*span));
                    model.decls[decl].mark = mark;
                    model.fields(&is_foreign, decl, "struct", name, fields);
                    if recursive.contains(name.as_str()) && !has_max_depth(parameters) {
                        let at = format!("struct `{name}`");
                        let id = model.elems.len();
                        model.elems.push(Elem {
                            at,
                            segment: name.clone(),
                            marker_on: format!("struct `{name}`"),
                            mark: None,
                            cause: Some(Cause::Depth),
                            names: vec![],
                            span: Some(*span),
                        });
                        model.decls[decl].elems.push(id);
                        model.owner.insert(id, decl);
                    }
                }
                FrozenUnit::Error {
                    name,
                    fields,
                    parameters,
                    span,
                    imported_from: None,
                    ..
                } => {
                    let decl = model.index[name];
                    let mark = model.mark(parameters, Scope::Type, name.clone(), *span);
                    model.decls[decl].mark = mark;
                    model.fields(&is_foreign, decl, "error", name, fields);
                }
                FrozenUnit::TypeAlias {
                    name,
                    target,
                    parameters,
                    span,
                    ..
                } => {
                    let decl = model.index[name];
                    let mark = model.mark(parameters, Scope::Type, name.clone(), Some(*span));
                    model.decls[decl].mark = mark;
                    let id = model.elem(
                        &is_foreign,
                        format!("type `{name}`"),
                        name.clone(),
                        format!("type `{name}`"),
                        None,
                        target,
                        Some(*span),
                    );
                    model.decls[decl].elems.push(id);
                    model.owner.insert(id, decl);
                }
                FrozenUnit::Protocol {
                    name,
                    functions,
                    parameters,
                    ..
                } => {
                    let mark = model.mark(parameters, Scope::Protocol, name.clone(), None);
                    let legacy = mark.is_none() && unverified::marker(parameters).is_some();
                    let mut protocol = Protocol {
                        name: name.clone(),
                        mark,
                        legacy,
                        functions: vec![],
                    };
                    for function in functions {
                        let FrozenUnit::Function {
                            name: f,
                            parameters,
                            arguments,
                            _return,
                            throws,
                            span,
                            ..
                        } = function
                        else {
                            continue;
                        };
                        let path = format!("{name}.{f}");
                        let mark =
                            model.mark(parameters, Scope::Function, path.clone(), Some(*span));
                        let mut elems = vec![];
                        for argument in arguments {
                            let segment = format!("{path}({})", argument.name);
                            let mark = model.mark(
                                &argument.parameters,
                                Scope::Argument,
                                segment.clone(),
                                Some(argument.span),
                            );
                            elems.push(model.elem(
                                &is_foreign,
                                format!(
                                    "protocol `{name}`, function `{f}`, argument `{}`",
                                    argument.name
                                ),
                                segment,
                                format!("argument `{}` of `{path}`", argument.name),
                                mark,
                                &argument.ty,
                                Some(argument.span),
                            ));
                        }
                        if let Some(ret) = _return {
                            elems.push(model.elem(
                                &is_foreign,
                                format!("protocol `{name}`, function `{f}`'s return"),
                                format!("{path} return"),
                                format!("function `{path}`"),
                                None,
                                ret,
                                Some(*span),
                            ));
                        }
                        // An error another schema declares travels in the
                        // reply like a return.
                        for error in imported_errors(units, throws) {
                            let ty = TypeRef::Named(error.to_string());
                            elems.push(model.elem(
                                &is_foreign,
                                format!("protocol `{name}`, function `{f}`, error `{error}`"),
                                format!("{path} throws {error}"),
                                format!("function `{path}`"),
                                None,
                                &ty,
                                Some(*span),
                            ));
                        }
                        protocol.functions.push(Func { mark, elems });
                    }
                    model.protocols.push(protocol);
                }
                _ => {}
            }
        }
        model
    }

    fn fields(
        &mut self,
        is_foreign: &dyn Fn(&str) -> bool,
        decl: usize,
        kind: &str,
        name: &str,
        fields: &[FrozenUnit],
    ) {
        for field in fields {
            let FrozenUnit::Field {
                name: f,
                ty,
                parameters,
                span,
                ..
            } = field
            else {
                continue;
            };
            let path = format!("{name}.{f}");
            let mark = self.mark(parameters, Scope::Field, path.clone(), Some(*span));
            let id = self.elem(
                is_foreign,
                format!("{kind} `{name}`, field `{f}`"),
                path,
                format!("field `{f}` of `{name}`"),
                mark,
                ty,
                Some(*span),
            );
            self.decls[decl].elems.push(id);
            self.owner.insert(id, decl);
        }
    }
}

/// One cause reached, by one path.
struct Found {
    elem: usize,
    path: String,
    /// The markers the path passes through, outermost first.
    cover: Vec<usize>,
}

struct Walk<'m> {
    model: &'m Model,
    visited: HashSet<(usize, Vec<usize>)>,
    found: Vec<Found>,
}

impl<'m> Walk<'m> {
    fn new(model: &'m Model) -> Self {
        Walk {
            model,
            visited: HashSet::new(),
            found: vec![],
        }
    }

    fn decl(&mut self, decl: usize, path: &mut Vec<String>, cover: &[usize]) {
        let mut cover = cover.to_vec();
        cover.extend(self.model.decls[decl].mark);
        if !self.visited.insert((decl, cover.clone())) {
            return;
        }
        let elems = self.model.decls[decl].elems.clone();
        for elem in elems {
            self.elem(elem, path, &cover);
        }
    }

    fn elem(&mut self, elem: usize, path: &mut Vec<String>, cover: &[usize]) {
        let model: &'m Model = self.model;
        let e = &model.elems[elem];
        let mut cover = cover.to_vec();
        cover.extend(e.mark);
        path.push(e.segment.clone());
        if e.cause.is_some() {
            self.found.push(Found {
                elem,
                path: path.join(" -> "),
                cover: cover.clone(),
            });
        }
        for name in &e.names {
            if let Some(&decl) = model.index.get(name) {
                self.decl(decl, path, &cover);
            }
        }
        path.pop();
    }

    fn protocol(&mut self, protocol: &Protocol) {
        let mut cover: Vec<usize> = vec![];
        if protocol.legacy {
            cover.push(LEGACY);
        }
        cover.extend(protocol.mark);
        for function in &protocol.functions {
            let mut cover = cover.clone();
            cover.extend(function.mark);
            for &elem in &function.elems {
                self.elem(elem, &mut vec![], &cover);
            }
        }
    }
}

/// How a cause is covered.
enum Class {
    Uncovered,
    /// Only by a protocol marker that doesn't list `bounded_size`.
    Legacy,
    /// By the innermost of the markers on its path, or by what the schema it
    /// comes from acknowledged.
    Covered(Vec<Acknowledgement>),
}

fn acknowledgement(mark: &Mark) -> Acknowledgement {
    Acknowledgement {
        guarantee: Guarantee::BoundedSize,
        scope: mark.scope,
        path: mark.path.clone(),
        reason: mark.reason.clone(),
    }
}

fn classify(model: &Model, found: &Found, foreign: &dyn Fn(&str) -> Foreign) -> Class {
    if let Some(Cause::Foreign(name)) = &model.elems[found.elem].cause {
        if let Foreign::Acknowledged(acks) = foreign(name) {
            return Class::Covered(acks);
        }
    }
    match found.cover.last() {
        None => Class::Uncovered,
        Some(&LEGACY) => Class::Legacy,
        Some(&mark) => Class::Covered(vec![acknowledgement(&model.marks[mark])]),
    }
}

fn push_unique(acks: &mut Vec<Acknowledgement>, more: Vec<Acknowledgement>) {
    for ack in more {
        if !acks.contains(&ack) {
            acks.push(ack);
        }
    }
}

/// Judge every unbounded position of `units`: which protocols give up
/// bounded size and where it's acknowledged, what no marker covers (errors),
/// and the markers that cover nothing (warnings). `foreign` says what another
/// schema's types hold.
pub fn analyze(units: &[FrozenUnit], foreign: &dyn Fn(&str) -> Foreign) -> Outcome {
    let model = Model::build(units, foreign);
    let mut outcome = Outcome::default();
    let mut touched: HashSet<usize> = HashSet::new();

    // Per position: the protocols that reach it by a path no marker covers
    // (with that path), and whether any protocol reaches it at all.
    let mut uncovered: HashMap<usize, Vec<(String, String)>> = HashMap::new();
    let mut reached: HashSet<usize> = HashSet::new();

    for protocol in &model.protocols {
        let mut walk = Walk::new(&model);
        walk.protocol(protocol);
        let mut acks: Vec<Acknowledgement> = vec![];
        let (mut gives_up, mut legacy) = (false, false);
        for found in &walk.found {
            reached.insert(found.elem);
            touched.extend(found.cover.iter().copied().filter(|m| *m != LEGACY));
            match classify(&model, found, foreign) {
                Class::Uncovered => {
                    let at = uncovered.entry(found.elem).or_default();
                    if !at.iter().any(|(p, _)| *p == protocol.name) {
                        at.push((protocol.name.clone(), found.path.clone()));
                    }
                }
                Class::Legacy => {
                    gives_up = true;
                    legacy = true;
                }
                Class::Covered(more) => {
                    gives_up = true;
                    push_unique(&mut acks, more);
                }
            }
        }
        if gives_up {
            let acknowledgements = if legacy { vec![] } else { acks };
            outcome
                .protocols
                .insert(protocol.name.clone(), Given { acknowledgements });
        }
    }

    // A declaration no protocol reaches is a type another package can use:
    // what it holds has to be bounded, unless a marker on it or what's under
    // it acknowledges it where it is.
    let mut unreached: HashMap<usize, String> = HashMap::new();
    for decl in 0..model.decls.len() {
        let mut walk = Walk::new(&model);
        walk.decl(decl, &mut vec![], &[]);
        for found in &walk.found {
            touched.extend(found.cover.iter().copied());
            let own = model.owner.get(&found.elem) == Some(&decl);
            if own
                && !reached.contains(&found.elem)
                && matches!(classify(&model, found, foreign), Class::Uncovered)
            {
                unreached
                    .entry(found.elem)
                    .or_insert_with(|| found.path.clone());
            }
        }
    }

    // Report in source order.
    for (id, elem) in model.elems.iter().enumerate() {
        let Some(cause) = &elem.cause else { continue };
        let by = uncovered.get(&id);
        let alone = unreached.get(&id);
        if by.is_none() && alone.is_none() {
            continue;
        }
        let path = by
            .and_then(|by| by.first())
            .map(|(_, path)| path.clone())
            .or_else(|| alone.cloned())
            .unwrap_or_default();
        let protocols: Vec<String> = by
            .into_iter()
            .flatten()
            .map(|(p, _)| format!("`{p}`"))
            .collect();
        let (code, message, help) = match cause {
            Cause::Depth => (
                UNBOUNDED_DEPTH,
                format!("{} can contain itself, and has no depth limit", elem.at),
                "write `@max_depth = N` on it, or set `bounds.depth.max` in settings".to_string(),
            ),
            Cause::Foreign(ty) => (
                UNBOUNDED,
                format!(
                    "{}: `{ty}` holds a value without a maximum, where it's declared",
                    elem.at
                ),
                format!("bound it where `{ty}` is declared"),
            ),
            Cause::Value(ty) => {
                let kind = if ty == "string" || ty.starts_with("char") {
                    "string"
                } else {
                    "array"
                };
                let written = if kind == "string" {
                    "char[max N]".to_string()
                } else {
                    format!("{}[max N]", ty.trim_end_matches("[]"))
                };
                (
                    UNBOUNDED,
                    format!("{}: `{ty}` has no maximum", elem.at),
                    format!("write `{written}`, or set `bounds.{kind}.max` in settings"),
                )
            }
        };
        let message = format!("{message} (reached through `{path}`)");
        // Where it's reached, bounded size could be given up: by the
        // protocol, or only where it's given up.
        let help = if protocols.is_empty() {
            help
        } else {
            format!(
                "{help}; or give up bounded size with `@unverified = \"…\"` on {}, or \
                 `@unverified(bounded_size = \"…\")` on {}",
                protocols.join(", "),
                elem.marker_on
            )
        };
        outcome.errors.push(
            Diagnostic::new(code, message)
                .with_help(help)
                .maybe_span(elem.span),
        );
    }

    // A marker below the protocol that no cause is reached through.
    for (id, mark) in model.marks.iter().enumerate() {
        if mark.scope != Scope::Protocol && !touched.contains(&id) {
            outcome.warnings.push(
                Diagnostic::new(
                    NOTHING_GIVEN_UP,
                    format!(
                        "`@unverified` on {} `{}` gives nothing up: nothing reached through it is without a maximum",
                        mark.scope.as_str(),
                        mark.path
                    ),
                )
                .with_help("remove the marker")
                .maybe_span(mark.span),
            );
        }
    }
    outcome
}

/// The declarations of `units` that hold a value without a maximum and
/// whose every such value a marker on the declaration, or under it,
/// acknowledges, each with where ([`Foreign::Acknowledged`]).
pub fn acknowledged_declarations(
    units: &[FrozenUnit],
    foreign: &dyn Fn(&str) -> Foreign,
) -> HashMap<String, Vec<Acknowledgement>> {
    let model = Model::build(units, foreign);
    let mut acknowledged = HashMap::new();
    for (name, &decl) in &model.index {
        let mut walk = Walk::new(&model);
        walk.decl(decl, &mut vec![], &[]);
        let mut acks = vec![];
        let mut all = !walk.found.is_empty();
        for found in &walk.found {
            match classify(&model, found, foreign) {
                Class::Covered(more) => push_unique(&mut acks, more),
                _ => all = false,
            }
        }
        if all {
            acknowledged.insert(name.clone(), acks);
        }
    }
    acknowledged
}
