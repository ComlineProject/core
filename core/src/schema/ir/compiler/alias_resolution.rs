//! Pre-freeze validation for `type` aliases (`type UserId = u64`) - fully
//! transparent, like Rust's own `type`, not a newtype wrapper.
//!
//! A `type` alias freezes to its own lightweight `FrozenUnit::TypeAlias`
//! (see `interpreter::incremental`), exactly the same way a struct or enum
//! declaration freezes to its own named unit - and a reference to the
//! alias elsewhere (a field, an argument, ...) freezes to an ordinary
//! `KindValue::Namespaced(name, _)`, exactly the same way a struct/enum
//! reference already does. No substitution happens anywhere; the alias's
//! name survives all the way to codegen, which needs only one new case
//! (emit the declaration itself) to support it correctly - every *use* of
//! the alias already works for free, through the same string-based type
//! mapping every generator already applies to any other `KindValue`.
//!
//! What *is* this module's job: a `type` alias is still required to make
//! sense before any of that happens - no name collision with another
//! declaration, no cycle among aliases, and every alias's target must
//! ultimately resolve to something real (a primitive, a local nominal
//! declaration, another valid alias, or - with `use_context` - a name
//! reachable through this schema's own `use` statements). None of this
//! can be caught after freezing: the `FrozenUnit`-based `SymbolTable` in
//! `validation::validator` only sees one flat list of names, with no
//! notion of "this name's own definition might itself be broken" - a
//! struct can be self-referential (through an array) but an alias cycle
//! or a dangling target is always a mistake, so it's checked explicitly,
//! here, before freezing ever runs.

use std::collections::{HashMap, HashSet};

use crate::package::config::ir::context::ProjectContext;
use crate::schema::idl::grammar::{Declaration, Type};
use crate::schema::ir::compiler::import_resolver::{find_schema_bringing_into_scope, schema_declares_symbol};
use crate::schema::ir::validation::validator::is_primitive;
use crate::diagnostics::Diagnostic;

/// Validate every local `type` alias in `declarations`: no name collision
/// with another declaration (local), no cycle among local aliases, and
/// every alias's target ultimately resolves to something real (a
/// primitive, a local struct/enum/const/protocol, another valid local
/// alias, or - with `use_context` - a name reachable through this
/// schema's own `use` statements).
pub fn check_aliases(
    declarations: &[rust_sitter::Spanned<Declaration>],
    use_context: Option<(&[String], &ProjectContext)>,
) -> Result<(), Vec<Diagnostic>> {
    let mut errors = Vec::new();

    // Every other top-level name a `type` alias could collide with.
    let mut other_names: HashMap<String, &'static str> = HashMap::new();
    for decl in declarations {
        let (name, kind) = match &decl.value {
            Declaration::Struct(s) => (s.name(), "struct"),
            Declaration::Enum(e) => (e.name(), "enum"),
            Declaration::Protocol(p) => (p.name(), "protocol"),
            Declaration::Const(c) => (c.name(), "const"),
            Declaration::Validator(v) => (v.name(), "validator"),
            _ => continue,
        };
        other_names.entry(name).or_insert(kind);
    }

    // Local `type` aliases, in declaration order, checked against
    // `other_names` and each other. Stops collecting on the first
    // duplicate for a given name (same stop-on-duplicate spirit as
    // `validator::validate`'s own pass 1) so a later step never has to
    // guess which of two same-named aliases was "the real one."
    let mut alias_map: HashMap<String, (Type, (usize, usize))> = HashMap::new();
    for decl in declarations {
        let Declaration::TypeAlias(alias) = &decl.value else {
            continue;
        };
        let name = alias.name();

        if let Some(kind) = other_names.get(&name) {
            errors.push(Diagnostic { help: None,
                message: format!("Duplicate definition of '{}'", name),
                context: format!("Definition of TypeAlias '{}' (already a {})", name, kind),
                span: Some(decl.span),
            });
            continue;
        }
        if alias_map.contains_key(&name) {
            errors.push(Diagnostic { help: None,
                message: format!("Duplicate definition of '{}'", name),
                context: format!("Definition of TypeAlias '{}'", name),
                span: Some(decl.span),
            });
            continue;
        }
        alias_map.insert(name, (alias.target_type().clone(), decl.span));
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    if let Some(error) = detect_alias_cycle(&alias_map) {
        return Err(vec![error]);
    }

    for (name, (ty, span)) in &alias_map {
        if let Some(bad) = first_unresolvable_name(ty, declarations, &alias_map, use_context) {
            errors.push(Diagnostic { help: None,
                message: format!("Type alias '{}' targets unknown type '{}'", name, bad),
                context: format!("Type alias '{}'", name),
                span: Some(*span),
            });
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Every type name a local alias's target directly mentions (recursing
/// into `Array`/`Union` to find a nested `Named`, but not chasing through
/// *other* aliases' own targets - the cycle DFS below does that itself).
fn direct_alias_refs(ty: &Type, alias_map: &HashMap<String, (Type, (usize, usize))>) -> Vec<String> {
    match ty {
        Type::Named(id) => {
            let full = id.to_string();
            let bare = full.rsplit("::").next().unwrap_or(&full).to_string();
            if alias_map.contains_key(&bare) {
                vec![bare]
            } else {
                vec![]
            }
        }
        Type::Array(arr) => direct_alias_refs(arr.elem_type(), alias_map),
        Type::Union(u) => u
            .members()
            .iter()
            .flat_map(|m| direct_alias_refs(m, alias_map))
            .collect(),
        _ => vec![],
    }
}

/// DFS cycle detection over the local alias graph (`type A = B` is an edge
/// `A -> B`). Same visited/visiting shape as
/// `package::config::ir::compiler::interpret::detect_import_cycle`'s own
/// `visit_for_cycle` and `validation::validator::detect_cycle`. Returns at
/// most one error - the first cycle found - naming the full cycle.
fn detect_alias_cycle(alias_map: &HashMap<String, (Type, (usize, usize))>) -> Option<Diagnostic> {
    let mut visited: HashSet<String> = HashSet::new();

    for start_name in alias_map.keys() {
        if visited.contains(start_name) {
            continue;
        }
        let mut visiting: HashSet<String> = HashSet::new();
        let mut path: Vec<String> = Vec::new();
        if let Some(cycle) = visit_alias(start_name, alias_map, &mut visited, &mut visiting, &mut path) {
            return Some(Diagnostic { help: None,
                message: format!("Cycle detected among type aliases: {}", cycle.join(" -> ")),
                context: format!("Type alias '{}'", start_name),
                span: alias_map.get(start_name.as_str()).map(|(_, span)| *span),
            });
        }
    }

    None
}

fn visit_alias(
    name: &str,
    alias_map: &HashMap<String, (Type, (usize, usize))>,
    visited: &mut HashSet<String>,
    visiting: &mut HashSet<String>,
    path: &mut Vec<String>,
) -> Option<Vec<String>> {
    if visiting.contains(name) {
        let start = path.iter().position(|n| n == name).unwrap_or(0);
        let mut cycle = path[start..].to_vec();
        cycle.push(name.to_string());
        return Some(cycle);
    }
    if visited.contains(name) {
        return None;
    }

    visiting.insert(name.to_string());
    path.push(name.to_string());

    if let Some((ty, _)) = alias_map.get(name) {
        for referenced in direct_alias_refs(ty, alias_map) {
            if let Some(cycle) = visit_alias(&referenced, alias_map, visited, visiting, path) {
                return Some(cycle);
            }
        }
    }

    path.pop();
    visiting.remove(name);
    visited.insert(name.to_string());
    None
}

/// Whether `declarations` locally declares `name` as a struct, enum, const,
/// or protocol - the same "a type reference to this name is meaningful"
/// set `validation::validator::validate_type` effectively accepts today
/// (via `SymbolTable`, which also lets a `Protocol` name through a type
/// position without complaint - matched here for consistency, not because
/// it's semantically meaningful to type a field as a protocol).
fn declares_nominal_name(declarations: &[rust_sitter::Spanned<Declaration>], name: &str) -> bool {
    declarations.iter().any(|d| match &d.value {
        Declaration::Struct(s) => s.name() == name,
        Declaration::Enum(e) => e.name() == name,
        Declaration::Const(c) => c.name() == name,
        Declaration::Protocol(p) => p.name() == name,
        _ => false,
    })
}

/// Find the first type name inside `ty` that can't be resolved to a
/// primitive, a local nominal declaration, another local alias, or a
/// `use`-imported name - `None` if everything bottoms out somewhere real.
/// Doesn't re-validate another local alias's own resolvability (that
/// alias's own entry in the loop in `check_aliases` already covers it,
/// and cycle detection has already proven the alias graph acyclic, so
/// there's no risk of this silently masking a real problem - it would
/// just be reported against the other alias's declaration instead).
fn first_unresolvable_name(
    ty: &Type,
    declarations: &[rust_sitter::Spanned<Declaration>],
    alias_map: &HashMap<String, (Type, (usize, usize))>,
    use_context: Option<(&[String], &ProjectContext)>,
) -> Option<String> {
    match ty {
        Type::Array(arr) => first_unresolvable_name(arr.elem_type(), declarations, alias_map, use_context),
        Type::Union(u) => u
            .members()
            .iter()
            .find_map(|m| first_unresolvable_name(m, declarations, alias_map, use_context)),
        Type::Named(id) => {
            let full = id.to_string();
            let bare = full.rsplit("::").next().unwrap_or(&full).to_string();

            if is_primitive(&bare) {
                return None;
            }
            if alias_map.contains_key(&bare) {
                return None;
            }
            if declares_nominal_name(declarations, &bare) {
                return None;
            }

            if let Some((current_namespace, project_context)) = use_context {
                if let Some(schema) =
                    find_schema_bringing_into_scope(&bare, declarations, current_namespace, project_context)
                {
                    let schema_ref = schema.borrow();
                    // Covers both a real nominal type and a foreign `type`
                    // alias (schema_declares_symbol matches both) - a
                    // foreign alias's own resolvability is that schema's
                    // own problem to validate, not re-checked here.
                    if schema_declares_symbol(&schema_ref, &bare) {
                        return None;
                    }
                }
            }

            Some(bare)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::idl::grammar;

    fn decls(source: &str) -> Vec<rust_sitter::Spanned<Declaration>> {
        grammar::parse(source).expect("should parse").0
    }

    #[test]
    fn duplicate_alias_and_struct_name_is_rejected() {
        let declarations = decls("struct User {\nid: u64\n}\ntype User = u64");
        let result = check_aliases(&declarations, None);
        assert!(result.is_err());
    }

    #[test]
    fn cycle_is_rejected() {
        let declarations = decls("type A = B\ntype B = A");
        let result = check_aliases(&declarations, None);
        let errors = result.expect_err("should detect a cycle");
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("Cycle detected"));
    }

    #[test]
    fn unresolvable_target_is_a_single_error() {
        let declarations = decls("type X = Nope\nstruct A {\na: X\n}\nstruct B {\nb: X\n}");
        let result = check_aliases(&declarations, None);
        let errors = result.expect_err("should flag the unresolvable target");
        assert_eq!(errors.len(), 1, "one error at the alias, not one per use site");
        assert!(errors[0].message.contains("Nope"));
    }

    #[test]
    fn valid_aliases_pass() {
        let declarations = decls("type UserId = u64\ntype Ids = UserId[]\nstruct S {\nid: UserId\n}");
        assert!(check_aliases(&declarations, None).is_ok());
    }
}
