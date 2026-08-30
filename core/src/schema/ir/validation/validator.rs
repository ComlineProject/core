// Standard Uses
use std::collections::{HashMap, HashSet};

// Crate Uses
use super::{ValidationError, symbols::{SymbolTable, SymbolType}};
use crate::schema::ir::frozen::unit::FrozenUnit;
use crate::schema::ir::compiler::interpreted::kind_search::KindValue;



pub fn validate(units: &[FrozenUnit]) -> Result<(), Vec<ValidationError>> {
    let mut errors = vec![];
    let mut symbols = SymbolTable::new();

    // Pass 1: Collect Symbols & Check Duplicates
    for unit in units {
        let (name, kind, span) = match unit {
            FrozenUnit::Struct { name, span, .. } => (name.as_str(), SymbolType::Struct, Some(*span)),
            FrozenUnit::Enum { name, span, .. } => (name.as_str(), SymbolType::Enum, Some(*span)),
            FrozenUnit::Protocol { name, span, .. } => (name.as_str(), SymbolType::Protocol, Some(*span)),
            FrozenUnit::Constant { name, span, .. } => (name.as_str(), SymbolType::Constant, Some(*span)),
            FrozenUnit::Import(path, span) => (path.as_str(), SymbolType::Import, Some(*span)),
            FrozenUnit::Validator { name, .. } => (name.as_str(), SymbolType::Validator, None),
            // TODO: Function handling if they become top-level
            _ => continue,
        };

        if let Err(existing_kind) = symbols.insert(name, kind) {
            // Two `use` paths naming the same symbol (e.g. a whole-namespace
            // import expanded alongside an explicit named import of one of
            // its symbols) is redundant, not a conflict - only a real
            // duplicate declaration, or an import colliding with one, is an
            // error.
            if kind == SymbolType::Import && existing_kind == SymbolType::Import {
                continue;
            }

            errors.push(ValidationError {
                message: format!("Duplicate definition of '{}'", name),
                context: format!("Definition of {:?} '{}'", kind, name),
                span,
            });
        }
    }

    // Stop if duplicate errors found (avoids cascading errors)
    if !errors.is_empty() {
        return Err(errors);
    }

    // Pass 2: Type Resolution & Usage
    for unit in units {
        match unit {
            FrozenUnit::Struct { name, fields, .. } => {
                for field in fields {
                    match field {
                        FrozenUnit::Field { name: field_name, kind_value, span, .. } => {
                            validate_type(kind_value, &symbols, &mut errors, &format!("Struct '{}', field '{}'", name, field_name), *span);
                        }
                        _ => {}
                    }
                }
            }
            FrozenUnit::Protocol { name, functions, .. } => {
                for func in functions {
                    match func {
                        FrozenUnit::Function { name: func_name, arguments, _return, span, .. } => {
                            for arg in arguments {
                                validate_type(&arg.kind, &symbols, &mut errors, &format!("Protocol '{}', function '{}', arg '{}'", name, func_name, arg.name), arg.span);
                            }
                            if let Some(ret_type) = _return {
                                validate_type(ret_type, &symbols, &mut errors, &format!("Protocol '{}', function '{}' return", name, func_name), *span);
                            }
                        }
                        _ => {}
                    }
                }
            }
            FrozenUnit::Constant { name, kind_value, span, .. } => {
                // Constants usually primitive, but check if namespaced
                if let KindValue::Namespaced(type_name, _) = kind_value {
                     errors.push(ValidationError {
                        message: format!("Constant '{}' cannot be a named type '{}' - only primitives allowed", name, type_name),
                        context: format!("Constant '{}'", name),
                        span: Some(*span),
                    });
                }
            }
            _ => {}
        }
    }

    // Pass 2.5: every `@validators = [Name(...)]` on a struct/error field must
    // name a declared `validator`, and (for locally declared ones) its keyword
    // arguments must be that validator's properties.
    let validator_props: HashMap<&str, Vec<&str>> = units
        .iter()
        .filter_map(|u| match u {
            FrozenUnit::Validator { name, properties, .. } => Some((
                name.as_str(),
                properties
                    .iter()
                    .filter_map(|p| match p {
                        FrozenUnit::Field { name, .. } => Some(name.as_str()),
                        _ => None,
                    })
                    .collect(),
            )),
            _ => None,
        })
        .collect();

    // Pass 2.6: a `validate {}` block references only `value.*` and this
    // validator's own `params.*`.
    for unit in units {
        let FrozenUnit::Validator { name: vname, expression_block, .. } = unit else {
            continue;
        };
        let props = validator_props
            .get(vname.as_str())
            .cloned()
            .unwrap_or_default();
        let FrozenUnit::ExpressionBlock { asserts } = &**expression_block else {
            continue;
        };
        let vctx = format!("Validator '{}'", vname);
        for a in asserts {
            let FrozenUnit::Assert { references, .. } = a else {
                continue;
            };
            for path in references {
                let mut segs = path.split('.');
                match segs.next().unwrap_or("") {
                    "value" => {}
                    "params" => {
                        if let Some(prop) = segs.next() {
                            if !props.contains(&prop) {
                                let mut message = format!(
                                    "validator '{}': unknown property 'params.{}'",
                                    vname, prop
                                );
                                if let Some(s) = closest(prop, props.iter().copied()) {
                                    message.push_str(
                                        &format!(" - did you mean 'params.{}'?", s),
                                    );
                                }
                                errors.push(ValidationError {
                                    message,
                                    context: vctx.clone(),
                                    span: None,
                                });
                            }
                        }
                    }
                    other => errors.push(ValidationError {
                        message: format!(
                            "validator '{}': `{}` is not a valid reference in `validate` \
                             (use `value.*` or `params.*`)",
                            vname, other
                        ),
                        context: vctx.clone(),
                        span: None,
                    }),
                }
            }
        }
    }

    for unit in units {
        let (owner_kind, owner_name, fields) = match unit {
            FrozenUnit::Struct { name, fields, .. } => ("Struct", name, fields),
            FrozenUnit::Error { name, fields, .. } => ("Error", name, fields),
            _ => continue,
        };
        for field in fields {
            let FrozenUnit::Field { name: field_name, parameters, span, .. } = field else {
                continue;
            };
            for param in parameters {
                let FrozenUnit::ValidatorRef { name: vname, args } = param else {
                    continue;
                };

                // Imports are keyed by their full `a::b::Name` path (or a
                // `a::b::*` glob); the other schema isn't loaded here, so any
                // import that could supply `vname` means we can't judge it.
                let maybe_imported = symbols.symbols.iter().any(|(k, v)| {
                    *v == SymbolType::Import
                        && (*k == vname
                            || k.ends_with(&format!("::{vname}"))
                            || k.ends_with("::*"))
                });

                let field_ctx =
                    format!("{} '{}', field '{}'", owner_kind, owner_name, field_name);

                match symbols.symbols.get(vname.as_str()) {
                    Some(SymbolType::Validator) => {
                        // Locally declared - check the keyword arguments.
                        if let Some(props) = validator_props.get(vname.as_str()) {
                            let mut seen: Vec<&str> = Vec::new();
                            for arg in args {
                                let FrozenUnit::Property { name: kw, .. } = arg else {
                                    continue;
                                };
                                if seen.contains(&kw.as_str()) {
                                    errors.push(ValidationError {
                                        message: format!(
                                            "duplicate argument '{}' to validator '{}'",
                                            kw, vname
                                        ),
                                        context: field_ctx.clone(),
                                        span: Some(*span),
                                    });
                                    continue;
                                }
                                seen.push(kw.as_str());
                                if !props.contains(&kw.as_str()) {
                                    let mut message = format!(
                                        "validator '{}' has no argument '{}'",
                                        vname, kw
                                    );
                                    if let Some(s) = closest(kw, props.iter().copied()) {
                                        message.push_str(
                                            &format!(" - did you mean '{}'?", s),
                                        );
                                    }
                                    errors.push(ValidationError {
                                        message,
                                        context: field_ctx.clone(),
                                        span: Some(*span),
                                    });
                                }
                            }
                        }
                    }
                    Some(other) if *other != SymbolType::Import => {
                        errors.push(ValidationError {
                            message: format!("'{}' is not a validator", vname),
                            context: format!(
                                "{} '{}', field '{}'",
                                owner_kind, owner_name, field_name
                            ),
                            span: Some(*span),
                        });
                    }
                    _ if maybe_imported => {}
                    _ => {
                        let mut message = format!("Unknown validator '{}'", vname);
                        if let Some(s) = suggest_similar_name(vname, &symbols) {
                            message.push_str(&format!(" - did you mean '{}'?", s));
                        }
                        errors.push(ValidationError {
                            message,
                            context: format!(
                                "{} '{}', field '{}'",
                                owner_kind, owner_name, field_name
                            ),
                            span: Some(*span),
                        });
                    }
                }
            }
        }
    }

    // Pass 3: Cycle Detection (Structs)
    let unit_map: HashMap<&str, &FrozenUnit> = units.iter().filter_map(|u| match u {
        FrozenUnit::Struct { name, .. } => Some((name.as_str(), u)),
        _ => None
    }).collect();

    let mut visited = HashSet::new();
    let mut visiting = HashSet::new();

    for (name, _) in &unit_map {
        if !visited.contains(name) {
            detect_cycle(name, &unit_map, &mut visited, &mut visiting, &mut errors);
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn detect_cycle<'a>(
    current: &'a str, 
    unit_map: &HashMap<&'a str, &'a FrozenUnit>, 
    visited: &mut HashSet<&'a str>, 
    visiting: &mut HashSet<&'a str>,
    errors: &mut Vec<ValidationError>
) {
    visiting.insert(current);
    
    if let Some(FrozenUnit::Struct { fields, span, .. }) = unit_map.get(current) {
        for field in fields {
            if let FrozenUnit::Field { kind_value, .. } = field {
                if let KindValue::Namespaced(type_name, _) = kind_value {
                    // Cycles are broken by dynamic arrays
                    if type_name.ends_with("[]") {
                        continue;
                    }
                    
                    // Handle fixed arrays [N] -> technically still a cycle
                    let base_type = type_name.split('[').next().unwrap_or(type_name);

                    if unit_map.contains_key(base_type) {
                        if visiting.contains(base_type) {
                            errors.push(ValidationError {
                                message: format!("Cycle detected involving struct '{}'", base_type),
                                context: format!("Struct '{}' depends on '{}'", current, base_type),
                                span: Some(*span),
                            });
                        } else if !visited.contains(base_type) {
                            detect_cycle(base_type, unit_map, visited, visiting, errors);
                        }
                    }
                }
            }
        }
    }

    visiting.remove(current);
    visited.insert(current);
}

fn validate_type(
    kind: &KindValue, symbols: &SymbolTable, errors: &mut Vec<ValidationError>,
    context: &str, span: (usize, usize),
) {
    match kind {
        KindValue::Namespaced(type_name, _) => {
            // Handle array syntax e.g. "User[]", "User[][]"
            let base_type = type_name.trim_end_matches("[]");
            
            // Allow primitives
            if is_primitive(base_type) {
                return;
            }

            // Check if type exists
            if !symbols.contains(base_type) {
                let mut message = format!("Unknown type '{}'", base_type);
                if let Some(suggestion) = suggest_similar_name(base_type, symbols) {
                    message.push_str(&format!(" - did you mean '{}'?", suggestion));
                }
                errors.push(ValidationError {
                    message,
                    context: context.to_string(),
                    span: Some(span),
                });
            }
        }
        KindValue::Primitive(_) => {
            // Primitives are always valid
        }
        KindValue::Union(members) => {
            for member in members {
                validate_type(member, symbols, errors, context, span);
            }
        }
        KindValue::EnumVariant(_, _) => {
            // KindValue::EnumVariant only carries the variant's own name, not
            // the enclosing enum's - there's no way to check it exists without
            // that context, and no grammar syntax produces this as a field's
            // type today (it's only ever used to represent one variant inside
            // an Enum's own `variants` list, not as a referenceable type).
        }
    }
}

const PRIMITIVE_NAMES: &[&str] = &[
    "bool",
    "u8", "u16", "u32", "u64", "u128",
    "s8", "s16", "s32", "s64", "s128",
    "f32", "f64",
    "str", "string",
];

fn is_primitive(name: &str) -> bool {
    PRIMITIVE_NAMES.contains(&name)
}

/// Closest candidate to `target` within a "plausibly a typo" edit distance
/// (half the target's length, min 1) — `None` if nothing is close.
fn closest<'a>(target: &str, candidates: impl Iterator<Item = &'a str>) -> Option<String> {
    let len = target.chars().count();
    let max_distance = ((len + 1) / 2).max(1);
    candidates
        .filter(|&c| c != target)
        .map(|c| (c, levenshtein_distance(target, c)))
        .filter(|&(_, d)| d <= max_distance)
        .min_by_key(|&(_, d)| d)
        .map(|(c, _)| c.to_string())
}

/// Standard Levenshtein edit distance (single-character insert/delete/
/// substitute) between two strings, used to power "did you mean?"
/// suggestions below.
fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let b_len = b.len();

    let mut prev: Vec<usize> = (0..=b_len).collect();
    let mut curr = vec![0usize; b_len + 1];

    for (i, &a_ch) in a.iter().enumerate() {
        curr[0] = i + 1;
        for (j, &b_ch) in b.iter().enumerate() {
            let cost = if a_ch == b_ch { 0 } else { 1 };
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[b_len]
}

/// Find the closest known symbol or primitive name to an unrecognized
/// type reference, for a "did you mean 'X'?" suggestion - `None` if
/// nothing is close enough to plausibly be a typo rather than just an
/// unrelated, genuinely nonexistent name.
fn suggest_similar_name(target: &str, symbols: &SymbolTable) -> Option<String> {
    // Allow up to half the target's length to differ (rounded up, at
    // least 1). A single transposition - swapping two adjacent letters,
    // e.g. "Uesr" -> "User" - already costs 2 under plain Levenshtein
    // (no dedicated swap operation), so a stricter ratio like 1/3 would
    // reject that exact typo; half the length comfortably covers it
    // while still excluding wildly different names.
    let len = target.chars().count();
    let max_distance = ((len + 1) / 2).max(1);

    symbols
        .symbols
        .keys()
        .copied()
        .chain(PRIMITIVE_NAMES.iter().copied())
        .filter(|&name| name != target)
        .map(|name| (name, levenshtein_distance(target, name)))
        .filter(|&(_, distance)| distance <= max_distance)
        .min_by_key(|&(_, distance)| distance)
        .map(|(name, _)| name.to_string())
}
