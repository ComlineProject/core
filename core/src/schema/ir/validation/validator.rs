// Standard Uses
use std::collections::{HashMap, HashSet};

// Local Uses
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
                errors.push(ValidationError {
                    message: format!("Unknown type '{}'", base_type),
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

fn is_primitive(name: &str) -> bool {
    matches!(name,
        "bool" | "u8" | "u16" | "u32" | "u64" | "u128" |
        "s8" | "s16" | "s32" | "s64" | "s128" |
        "f32" | "f64" | "str" | "string"
    )
}
