//! Resolution for `type` aliases (`type UserId = u64`) - fully transparent,
//! like Rust's `type`, not a newtype wrapper. An alias is erased entirely
//! before freezing: every `Type::Named` occurrence of it is substituted
//! with its target type before any `KindValue` is built, so the alias
//! declaration itself never produces a `FrozenUnit` and nothing downstream
//! (validation, codegen) ever needs to know it existed.
//!
//! This module has two halves:
//! - [`check_aliases`] - duplicate/cycle/unresolvable-target diagnostics,
//!   run once per schema before any substitution happens.
//! - [`resolve_type`] - the substitution itself, called at every type-use
//!   site in `interpreter::incremental` before `build_kind_value`/
//!   `type_to_kind_value` run.
//!
//! An erased alias never reaches the `FrozenUnit`-based `SymbolTable` that
//! `validation::validator` builds, so neither its duplicate-name check nor
//! its struct-cycle `detect_cycle` can catch a bad alias - both concerns
//! are this module's own responsibility instead.

use std::collections::{HashMap, HashSet};

use crate::package::config::ir::context::ProjectContext;
use crate::schema::idl::grammar::{self, Declaration, Type, TypeAlias};
use crate::schema::ir::compiler::import_resolver::{find_schema_bringing_into_scope, schema_declares_symbol};
use crate::schema::ir::validation::validator::is_primitive;
use crate::schema::ir::validation::ValidationError;

/// Defensive recursion cap for [`resolve_type`] - [`check_aliases`] rejects
/// real cycles before substitution ever runs, so this should never bind in
/// practice. It exists only so a caller that skips `check_aliases` (there
/// is one: `IncrementalInterpreter::compile_declarations`'s plain,
/// non-checking entry point) degrades to "leave unresolved" rather than
/// hanging on a cyclic alias.
const MAX_ALIAS_DEPTH: u32 = 64;

/// Validate every local `type` alias in `declarations`: no name collision
/// with another declaration (local or, via `other_names`, nothing further -
/// cross-file collisions aren't meaningful, since importing a name that
/// collides with a local one is already an existing "shadowing" case
/// `SymbolTable`/`bare_imports` handles elsewhere), no cycle among local
/// aliases, and every alias's target ultimately resolves to something real
/// (a primitive, a local struct/enum/const/protocol, another valid local
/// alias, or - with `use_context` - a name reachable through this schema's
/// own `use` statements).
pub fn check_aliases(
    declarations: &[rust_sitter::Spanned<Declaration>],
    use_context: Option<(&[String], &ProjectContext)>,
) -> Result<(), Vec<ValidationError>> {
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
            errors.push(ValidationError {
                message: format!("Duplicate definition of '{}'", name),
                context: format!("Definition of TypeAlias '{}' (already a {})", name, kind),
                span: Some(decl.span),
            });
            continue;
        }
        if alias_map.contains_key(&name) {
            errors.push(ValidationError {
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
            errors.push(ValidationError {
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
fn detect_alias_cycle(alias_map: &HashMap<String, (Type, (usize, usize))>) -> Option<ValidationError> {
    let mut visited: HashSet<String> = HashSet::new();

    for start_name in alias_map.keys() {
        if visited.contains(start_name) {
            continue;
        }
        let mut visiting: HashSet<String> = HashSet::new();
        let mut path: Vec<String> = Vec::new();
        if let Some(cycle) = visit_alias(start_name, alias_map, &mut visited, &mut visiting, &mut path) {
            return Some(ValidationError {
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

/// Find a local `type` alias declared by `declarations`.
fn find_local_alias<'a>(
    declarations: &'a [rust_sitter::Spanned<Declaration>],
    name: &str,
) -> Option<&'a TypeAlias> {
    declarations.iter().find_map(|d| match &d.value {
        Declaration::TypeAlias(t) if t.name() == name => Some(t),
        _ => None,
    })
}

/// Substitute every `Type::Named` occurrence of a `type` alias with its
/// resolved target, recursively (an alias may target another alias; an
/// array/union containing an alias is resolved member-wise). Cross-file:
/// if a name isn't a local alias, and `use_context` is `Some`, this walks
/// `declarations`' own `use` statements to find a foreign schema bringing
/// that name into scope - if the foreign declaration is itself a `type`
/// alias, its target is cloned and resolution continues **in the foreign
/// schema's own context** (its own locals/`use`s); if it's a real
/// struct/enum/const, the reference is left unchanged (today's by-name
/// cross-file behavior for genuine nominal types - not inlined).
///
/// Call this on every `grammar::Type` immediately before building a
/// `KindValue` from it (`build_kind_value`/`type_to_kind_value` in
/// `interpreter::incremental`) - never after.
pub fn resolve_type(
    ty: &Type,
    declarations: &[rust_sitter::Spanned<Declaration>],
    use_context: Option<(&[String], &ProjectContext)>,
) -> Type {
    resolve_type_inner(ty, declarations, use_context, 0)
}

fn resolve_type_inner(
    ty: &Type,
    declarations: &[rust_sitter::Spanned<Declaration>],
    use_context: Option<(&[String], &ProjectContext)>,
    depth: u32,
) -> Type {
    if depth >= MAX_ALIAS_DEPTH {
        return ty.clone();
    }

    match ty {
        Type::Named(id) => {
            let full = id.to_string();
            let bare = full.rsplit("::").next().unwrap_or(&full).to_string();

            if let Some(alias) = find_local_alias(declarations, &bare) {
                return resolve_type_inner(alias.target_type(), declarations, use_context, depth + 1);
            }

            if let Some((current_namespace, project_context)) = use_context {
                if let Some(schema) =
                    find_schema_bringing_into_scope(&bare, declarations, current_namespace, project_context)
                {
                    let schema_ref = schema.borrow();
                    if let Some(alias) = find_local_alias(&schema_ref.declarations, &bare) {
                        return resolve_type_inner(
                            alias.target_type(),
                            &schema_ref.declarations,
                            Some((&schema_ref.namespace, project_context)),
                            depth + 1,
                        );
                    }
                    // A real struct/enum/const (or genuinely unresolvable) -
                    // don't inline; fall through to the unchanged clone.
                }
            }

            ty.clone()
        }
        Type::Array(arr) => Type::Array(Box::new(arr.with_key(resolve_type_inner(
            arr.elem_type(),
            declarations,
            use_context,
            depth + 1,
        )))),
        Type::Union(u) => Type::Union(u.with_members(
            u.members()
                .iter()
                .map(|m| resolve_type_inner(m, declarations, use_context, depth + 1))
                .collect(),
        )),
        _ => ty.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decls(source: &str) -> Vec<rust_sitter::Spanned<Declaration>> {
        grammar::parse(source).expect("should parse").0
    }

    #[test]
    fn simple_alias_resolves() {
        let declarations = decls("type UserId = u64");
        let ty = Type::Named(grammar::ScopedIdentifier { text: "UserId".to_string() });
        let resolved = resolve_type(&ty, &declarations, None);
        assert!(matches!(resolved, Type::U64(_)));
    }

    #[test]
    fn alias_chain_resolves_to_final_target() {
        let declarations = decls("type A = B\ntype B = u32");
        let ty = Type::Named(grammar::ScopedIdentifier { text: "A".to_string() });
        let resolved = resolve_type(&ty, &declarations, None);
        assert!(matches!(resolved, Type::U32(_)));
    }

    #[test]
    fn alias_inside_array_resolves() {
        let declarations = decls("type Id = u64");
        let source = "struct S {\nids: Id[]\n}";
        let full = decls(source);
        // Reach into the struct's own field type via the parsed AST, same
        // as `incremental.rs` would.
        let Declaration::Struct(s) = &full[0].value else { panic!() };
        let field_ty = s.fields()[0].field_type();
        let resolved = resolve_type(field_ty, &declarations, None);
        match resolved {
            Type::Array(arr) => assert!(matches!(arr.elem_type(), Type::U64(_))),
            other => panic!("expected array, got {:?}", other),
        }
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
