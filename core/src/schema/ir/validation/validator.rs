use super::{ValidationError, symbols::{SymbolTable, SymbolType}};
use crate::schema::ir::frozen::unit::FrozenUnit;
use crate::schema::ir::compiler::interpreted::kind_search::KindValue;
use std::collections::{HashMap, HashSet};

pub fn validate(units: &[FrozenUnit]) -> Result<(), Vec<ValidationError>> {
    let mut errors = vec![];
    let mut symbols = SymbolTable::new();

    // Pass 1: Collect Symbols & Check Duplicates
    for unit in units {
        let (name, kind) = match unit {
            FrozenUnit::Struct { name, .. } => (name.as_str(), SymbolType::Struct),
            FrozenUnit::Enum { name, .. } => (name.as_str(), SymbolType::Enum),
            FrozenUnit::Protocol { name, .. } => (name.as_str(), SymbolType::Protocol),
            FrozenUnit::Constant { name, .. } => (name.as_str(), SymbolType::Constant),
            FrozenUnit::Import(path) => (path.as_str(), SymbolType::Import),
            // TODO: Function handling if they become top-level
            _ => continue,
        };

        if let Err(_existing_kind) = symbols.insert(name, kind) {
            errors.push(ValidationError {
                message: format!("Duplicate definition of '{}'", name),
                context: format!("Definition of {:?} '{}'", kind, name),
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
                        FrozenUnit::Field { name: field_name, kind_value, .. } => {
                            validate_type(kind_value, &symbols, &mut errors, &format!("Struct '{}', field '{}'", name, field_name));
                        }
                        _ => {}
                    }
                }
            }
            FrozenUnit::Protocol { name, functions, .. } => {
                for func in functions {
                    match func {
                        FrozenUnit::Function { name: func_name, arguments, _return, .. } => {
                            for arg in arguments {
                                validate_type(&arg.kind, &symbols, &mut errors, &format!("Protocol '{}', function '{}', arg '{}'", name, func_name, arg.name));
                            }
                            if let Some(ret_type) = _return {
                                validate_type(ret_type, &symbols, &mut errors, &format!("Protocol '{}', function '{}' return", name, func_name));
                            }
                        }
                        _ => {}
                    }
                }
            }
            FrozenUnit::Constant { name, kind_value, .. } => {
                // Constants usually primitive, but check if namespaced
                if let KindValue::Namespaced(type_name, _) = kind_value {
                     errors.push(ValidationError {
                        message: format!("Constant '{}' cannot be a named type '{}' - only primitives allowed", name, type_name),
                        context: format!("Constant '{}'", name),
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
    
    if let Some(FrozenUnit::Struct { fields, .. }) = unit_map.get(current) {
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

fn validate_type(kind: &KindValue, symbols: &SymbolTable, errors: &mut Vec<ValidationError>, context: &str) {
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
                });
            }
        }
        KindValue::Primitive(_) => {
            // Primitives are always valid
        }
        KindValue::EnumVariant(_, _) | KindValue::Union(_) => {
            // TODO: Implement validation for these types if they are used
        }
    }
}

fn is_primitive(name: &str) -> bool {
    matches!(name, 
        "bool" | "u8" | "u16" | "u32" | "u64" | "u128" | 
        "i8" | "i16" | "i32" | "i64" | "i128" | 
        "f32" | "f64" | "str" | "string"
    )
}
