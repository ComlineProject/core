// Schema diffing and change analysis

use crate::schema::ir::compiler::interpreted::kind_search::KindValue;
use crate::schema::ir::frozen::unit::FrozenUnit;
use std::collections::{HashMap, HashSet};

/// Structured representation of schema changes between two versions
#[derive(Debug, Clone, Default)]
pub struct SchemaChanges {
    pub breaking_changes: Vec<BreakingChange>,
    pub new_features: Vec<NewFeature>,
    pub modifications: Vec<Modification>,
}

impl SchemaChanges {
    pub fn is_empty(&self) -> bool {
        self.breaking_changes.is_empty()
            && self.new_features.is_empty()
            && self.modifications.is_empty()
    }

    pub fn is_breaking(&self) -> bool {
        !self.breaking_changes.is_empty()
    }

    pub fn is_feature(&self) -> bool {
        !self.new_features.is_empty()
    }
}

/// Breaking changes that require a major version bump
#[derive(Debug, Clone)]
pub enum BreakingChange {
    RemovedStruct {
        name: String,
    },
    RemovedEnum {
        name: String,
    },
    RemovedField {
        type_name: String,
        field_name: String,
    },
    ChangedFieldType {
        type_name: String,
        field_name: String,
        old_type: String,
        new_type: String,
    },
    RemovedEnumVariant {
        enum_name: String,
        variant: String,
    },
    RemovedFunction {
        protocol_name: String,
        function_name: String,
    },
    ChangedFunctionSignature {
        protocol_name: String,
        function_name: String,
        details: String,
    },
    RemovedProtocol {
        name: String,
    },
}

/// New features that require a minor version bump
#[derive(Debug, Clone)]
pub enum NewFeature {
    AddedStruct {
        name: String,
        field_count: usize,
    },
    AddedEnum {
        name: String,
        variant_count: usize,
    },
    AddedField {
        type_name: String,
        field_name: String,
        field_type: String,
        optional: bool,
    },
    AddedEnumVariant {
        enum_name: String,
        variant: String,
    },
    AddedFunction {
        protocol_name: String,
        function_name: String,
        signature: String,
    },
    AddedProtocol {
        name: String,
        function_count: usize,
    },
}

/// Non-breaking modifications that may warrant a patch bump
#[derive(Debug, Clone)]
pub enum Modification {
    FieldMadeOptional {
        type_name: String,
        field_name: String,
    },
    // Future: documentation changes, metadata updates, etc.
}

/// Analyze changes between two schema versions
pub fn analyze_schema_changes(
    old_schema: &[FrozenUnit],
    new_schema: &[FrozenUnit],
) -> SchemaChanges {
    let mut changes = SchemaChanges::default();

    // Build indices for efficient lookup
    let old_index = build_index(old_schema);
    let new_index = build_index(new_schema);

    // Check for removed declarations (breaking)
    for (name, old_unit) in &old_index {
        if !new_index.contains_key(name) {
            match old_unit {
                FrozenUnit::Struct { name, .. } => {
                    changes
                        .breaking_changes
                        .push(BreakingChange::RemovedStruct { name: name.clone() });
                }
                FrozenUnit::Enum { name, .. } => {
                    changes
                        .breaking_changes
                        .push(BreakingChange::RemovedEnum { name: name.clone() });
                }
                FrozenUnit::Protocol { name, .. } => {
                    changes
                        .breaking_changes
                        .push(BreakingChange::RemovedProtocol { name: name.clone() });
                }
                _ => {}
            }
        }
    }

    // Check for added declarations (features)
    for (name, new_unit) in &new_index {
        if !old_index.contains_key(name) {
            match new_unit {
                FrozenUnit::Struct { name, fields, .. } => {
                    changes.new_features.push(NewFeature::AddedStruct {
                        name: name.clone(),
                        field_count: fields.len(),
                    });
                }
                FrozenUnit::Enum { name, variants, .. } => {
                    changes.new_features.push(NewFeature::AddedEnum {
                        name: name.clone(),
                        variant_count: variants.len(),
                    });
                }
                FrozenUnit::Protocol {
                    name, functions, ..
                } => {
                    changes.new_features.push(NewFeature::AddedProtocol {
                        name: name.clone(),
                        function_count: functions.len(),
                    });
                }
                _ => {}
            }
        }
    }

    // Check for modifications to existing declarations
    for (name, new_unit) in &new_index {
        if let Some(old_unit) = old_index.get(name) {
            match (old_unit, new_unit) {
                (
                    FrozenUnit::Struct {
                        name,
                        fields: old_fields,
                        ..
                    },
                    FrozenUnit::Struct {
                        fields: new_fields, ..
                    },
                ) => {
                    compare_struct_fields(name, old_fields, new_fields, &mut changes);
                }
                (
                    FrozenUnit::Enum {
                        name,
                        variants: old_variants,
                        ..
                    },
                    FrozenUnit::Enum {
                        variants: new_variants,
                        ..
                    },
                ) => {
                    compare_enum_variants(name, old_variants, new_variants, &mut changes);
                }
                (
                    FrozenUnit::Protocol {
                        name,
                        functions: old_funcs,
                        ..
                    },
                    FrozenUnit::Protocol {
                        functions: new_funcs,
                        ..
                    },
                ) => {
                    compare_protocol_functions(name, old_funcs, new_funcs, &mut changes);
                }
                _ => {}
            }
        }
    }

    changes
}

fn build_index(schema: &[FrozenUnit]) -> HashMap<String, &FrozenUnit> {
    let mut index = HashMap::new();
    for unit in schema {
        let name = match unit {
            FrozenUnit::Struct { name, .. } => Some(name.clone()),
            FrozenUnit::Enum { name, .. } => Some(name.clone()),
            FrozenUnit::Protocol { name, .. } => Some(name.clone()),
            FrozenUnit::Constant { name, .. } => Some(name.clone()),
            _ => None,
        };
        if let Some(n) = name {
            index.insert(n, unit);
        }
    }
    index
}

fn compare_struct_fields(
    struct_name: &str,
    old_fields: &[FrozenUnit],
    new_fields: &[FrozenUnit],
    changes: &mut SchemaChanges,
) {
    let old_field_map = build_field_map(old_fields);
    let new_field_map = build_field_map(new_fields);

    // Removed fields (breaking)
    for (field_name, (_, old_optional)) in &old_field_map {
        if !new_field_map.contains_key(field_name) && !old_optional {
            changes.breaking_changes.push(BreakingChange::RemovedField {
                type_name: struct_name.to_string(),
                field_name: field_name.clone(),
            });
        }
    }

    // Added fields (feature if optional, breaking if required)
    for (field_name, (field_type, new_optional)) in &new_field_map {
        if !old_field_map.contains_key(field_name) {
            changes.new_features.push(NewFeature::AddedField {
                type_name: struct_name.to_string(),
                field_name: field_name.clone(),
                field_type: kind_to_string(field_type),
                optional: *new_optional,
            });
        }
    }

    // Changed field types (breaking)
    for (field_name, (new_type, _)) in &new_field_map {
        if let Some((old_type, _)) = old_field_map.get(field_name) {
            if !types_compatible(old_type, new_type) {
                changes
                    .breaking_changes
                    .push(BreakingChange::ChangedFieldType {
                        type_name: struct_name.to_string(),
                        field_name: field_name.clone(),
                        old_type: kind_to_string(old_type),
                        new_type: kind_to_string(new_type),
                    });
            }
        }
    }
}

fn build_field_map(fields: &[FrozenUnit]) -> HashMap<String, (KindValue, bool)> {
    let mut map = HashMap::new();
    for field in fields {
        if let FrozenUnit::Field {
            name,
            kind_value,
            optional,
            ..
        } = field
        {
            map.insert(name.clone(), (kind_value.clone(), *optional));
        }
    }
    map
}

fn compare_enum_variants(
    enum_name: &str,
    old_variants: &[FrozenUnit],
    new_variants: &[FrozenUnit],
    changes: &mut SchemaChanges,
) {
    let old_names: HashSet<String> = extract_variant_names(old_variants);
    let new_names: HashSet<String> = extract_variant_names(new_variants);

    // Removed variants (breaking)
    for variant in old_names.difference(&new_names) {
        changes
            .breaking_changes
            .push(BreakingChange::RemovedEnumVariant {
                enum_name: enum_name.to_string(),
                variant: variant.clone(),
            });
    }

    // Added variants (feature)
    for variant in new_names.difference(&old_names) {
        changes.new_features.push(NewFeature::AddedEnumVariant {
            enum_name: enum_name.to_string(),
            variant: variant.clone(),
        });
    }
}

fn extract_variant_names(variants: &[FrozenUnit]) -> HashSet<String> {
    variants
        .iter()
        .filter_map(|v| {
            if let FrozenUnit::EnumVariant(kv) = v {
                match kv {
                    KindValue::EnumVariant(name, _) => Some(name.clone()),
                    KindValue::Namespaced(name, _) => Some(name.clone()),
                    _ => None,
                }
            } else {
                None
            }
        })
        .collect()
}

fn compare_protocol_functions(
    protocol_name: &str,
    old_funcs: &[FrozenUnit],
    new_funcs: &[FrozenUnit],
    changes: &mut SchemaChanges,
) {
    let old_func_map = build_function_map(old_funcs);
    let new_func_map = build_function_map(new_funcs);

    // Removed functions (breaking)
    for func_name in old_func_map.keys() {
        if !new_func_map.contains_key(func_name) {
            changes
                .breaking_changes
                .push(BreakingChange::RemovedFunction {
                    protocol_name: protocol_name.to_string(),
                    function_name: func_name.clone(),
                });
        }
    }

    // Added functions (feature)
    for (func_name, sig) in &new_func_map {
        if !old_func_map.contains_key(func_name) {
            changes.new_features.push(NewFeature::AddedFunction {
                protocol_name: protocol_name.to_string(),
                function_name: func_name.clone(),
                signature: sig.clone(),
            });
        }
    }

    // Changed function signatures (breaking)
    for (func_name, new_sig) in &new_func_map {
        if let Some(old_sig) = old_func_map.get(func_name) {
            if old_sig != new_sig {
                changes
                    .breaking_changes
                    .push(BreakingChange::ChangedFunctionSignature {
                        protocol_name: protocol_name.to_string(),
                        function_name: func_name.clone(),
                        details: format!("{} → {}", old_sig, new_sig),
                    });
            }
        }
    }
}

fn build_function_map(functions: &[FrozenUnit]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for func in functions {
        if let FrozenUnit::Function {
            name,
            arguments,
            _return,
            ..
        } = func
        {
            let sig = format_function_signature(arguments, _return);
            map.insert(name.clone(), sig);
        }
    }
    map
}

fn format_function_signature(
    args: &[crate::schema::ir::frozen::unit::FrozenArgument],
    ret: &Option<KindValue>,
) -> String {
    let arg_types: Vec<String> = args.iter().map(|arg| kind_to_string(&arg.kind)).collect();

    let ret_type = ret
        .as_ref()
        .map(kind_to_string)
        .unwrap_or_else(|| "void".to_string());

    format!("({}) -> {}", arg_types.join(", "), ret_type)
}

fn kind_to_string(kind: &KindValue) -> String {
    match kind {
        KindValue::Primitive(p) => p.name().to_string(),
        KindValue::Namespaced(name, _) => name.clone(),
        KindValue::EnumVariant(name, _) => name.clone(),
        KindValue::Union(_) => "union".to_string(),
    }
}

fn types_compatible(old: &KindValue, new: &KindValue) -> bool {
    // For now, require exact match
    // Future: could allow compatible widening (e.g., u32 -> u64)
    kind_to_string(old) == kind_to_string(new)
}

// ===== Backward Compatibility =====
// The Differ trait is used by the (incomplete) autodoc system
// We keep it here for compatibility, though it's not used by the new diff system

/// Legacy trait for diffing operations (used by autodoc)
#[allow(unused)]
pub trait Differ {
    fn on_namespace_changed(&mut self, old: &str, new: &str);
    fn on_const_name_changed(&mut self, old: &str, new: &str);
    fn on_const_kind_changed(&mut self, old: u8, new: u8);
    fn on_const_default_value_changed(
        &mut self,
        name: &str,
        left_kind_value: &KindValue,
        right_kind_value: &KindValue,
    );
}
