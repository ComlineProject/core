// Schema diffing and change analysis

use crate::schema::ir::frozen::types::{Literal, TypeRef};
use crate::schema::ir::frozen::unit::{
    schema_namespace, ConfigValue, DeprecatedValue, FrozenArgument, FrozenUnit, Guarantee,
};
use crate::schema::ir::unverified;
use std::collections::{BTreeMap, HashMap};

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

    /// Append everything `other` found.
    pub fn extend(&mut self, other: SchemaChanges) {
        self.breaking_changes.extend(other.breaking_changes);
        self.new_features.extend(other.new_features);
        self.modifications.extend(other.modifications);
    }
}

/// The changes inside one dependency, between the version this package
/// built against last time and now.
#[derive(Debug, Clone)]
pub struct DependencyChanges {
    pub name: String,
    pub changes: SchemaChanges,
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
    /// A field disappeared, optional or not: its name is gone for every
    /// consumer that reads it.
    RemovedField {
        type_name: String,
        field_name: String,
    },
    AddedRequiredField {
        type_name: String,
        field_name: String,
        field_type: String,
    },
    ChangedFieldType {
        type_name: String,
        field_name: String,
        old_type: String,
        new_type: String,
    },
    /// `optional` flipped, either way. Made optional: old readers may now
    /// receive nothing where they expect a value. Made required: old writers
    /// may still leave it out.
    ChangedFieldOptionality {
        type_name: String,
        field_name: String,
        now_optional: bool,
    },
    RemovedEnumVariant {
        enum_name: String,
        variant: String,
    },
    /// One variant removed and one added at the same position: reported as
    /// the rename it most likely is, rather than a removal plus an addition.
    /// Still breaking, since variants travel by name.
    RenamedEnumVariant {
        enum_name: String,
        old_name: String,
        new_name: String,
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
    /// A function no longer declares an error it used to (`! Name`): the
    /// generated error variant disappears for its callers.
    RemovedThrownError {
        protocol_name: String,
        function_name: String,
        error: String,
    },
    RemovedProtocol {
        name: String,
    },
    RemovedError {
        name: String,
    },
    /// The error's ordinal is what travels on the wire, and it shifts when a
    /// local error is inserted, removed or reordered before it.
    ChangedErrorOrdinal {
        name: String,
        old_ordinal: u16,
        new_ordinal: u16,
    },
    RemovedConstant {
        name: String,
    },
    ChangedConstantType {
        name: String,
        old_type: String,
        new_type: String,
    },
    RemovedTypeAlias {
        name: String,
    },
    ChangedTypeAliasTarget {
        name: String,
        old_target: String,
        new_target: String,
    },
    RemovedValidator {
        name: String,
    },
    /// A validator declaration's targets, properties or body changed. What
    /// it accepts can't be compared in general, so any change counts as
    /// narrowing.
    ChangedValidator {
        name: String,
    },
    /// The validators applied to a declaration changed other than by pure
    /// removal: what's accepted may have narrowed.
    ValidationTightened {
        target: String,
    },
    /// An annotation that changes what travels on the wire (`@framing`).
    ChangedWireAnnotation {
        target: String,
        key: String,
        old_value: Option<String>,
        new_value: Option<String>,
    },
    /// A protocol gave up guarantees it used to keep (`@unverified`, or its
    /// set growing): peers relied on them.
    GaveUpGuarantees {
        target: String,
        guarantees: Vec<Guarantee>,
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
    /// A function declares a new error (`! Name`). Older callers see it as
    /// a remote error they don't know.
    AddedThrownError {
        protocol_name: String,
        function_name: String,
        error: String,
    },
    AddedProtocol {
        name: String,
        function_count: usize,
    },
    AddedError {
        name: String,
        field_count: usize,
    },
    AddedConstant {
        name: String,
        ty: String,
    },
    /// A constant's value changed. Counted as minor for now: whether it
    /// reaches the wire (a validator argument, a default) isn't traced yet.
    ChangedConstantValue {
        name: String,
        old_value: Option<String>,
        new_value: Option<String>,
    },
    AddedTypeAlias {
        name: String,
        target: String,
    },
    AddedValidator {
        name: String,
    },
    /// Validators were only removed from a declaration: what's accepted
    /// widens.
    ValidationLoosened {
        target: String,
    },
    /// `@deprecated` was added to a declaration.
    Deprecated {
        target: String,
    },
    /// A protocol keeps guarantees it used to give up.
    RestoredGuarantees {
        target: String,
        guarantees: Vec<Guarantee>,
    },
}

/// Non-breaking modifications that may warrant a patch bump
#[derive(Debug, Clone)]
pub enum Modification {
    /// A declaration's own docstring text changed with nothing else about
    /// it - doc/comment-only changes warrant a patch bump, not a
    /// major/minor one. Only tracked at the declaration level (struct,
    /// enum, protocol, const, error, alias, validator) for now, not
    /// per-field/per-function.
    DocstringChanged {
        name: String,
    },
    ChangedFieldDefault {
        type_name: String,
        field_name: String,
    },
    ChangedErrorMessage {
        name: String,
    },
    /// An annotation that doesn't change the wire changed: `@timeout_ms`,
    /// `@idempotent`, `@deprecated` removed or reworded, or one core doesn't
    /// know.
    ChangedAnnotation {
        target: String,
        key: String,
        old_value: Option<String>,
        new_value: Option<String>,
    },
}

/// Analyze changes between two versions of a package: its schema files,
/// paired by namespace.
///
/// A file whose namespace exists only in `old` is compared against nothing,
/// so each of its declarations is a removal; one only in `new`, an addition.
/// Pairing by namespace rather than by position means adding, renaming or
/// reordering files doesn't pair the wrong schemas.
pub fn analyze_package_changes(
    old_schemas: &[Vec<FrozenUnit>],
    new_schemas: &[Vec<FrozenUnit>],
) -> SchemaChanges {
    let old = by_namespace(old_schemas);
    let new = by_namespace(new_schemas);

    let mut changes = SchemaChanges::default();
    for (namespace, old_units) in &old {
        let new_units = new.get(namespace).copied().unwrap_or(&[]);
        changes.extend(analyze_schema_changes(old_units, new_units));
    }
    for (namespace, new_units) in &new {
        if !old.contains_key(namespace) {
            changes.extend(analyze_schema_changes(&[], new_units));
        }
    }
    changes
}

/// Each schema keyed by its namespace. One without a namespace keeps its
/// position, the best there is.
fn by_namespace(schemas: &[Vec<FrozenUnit>]) -> BTreeMap<String, &[FrozenUnit]> {
    schemas
        .iter()
        .enumerate()
        .map(|(i, units)| {
            let key = schema_namespace(units)
                .map(str::to_string)
                .unwrap_or_else(|| format!("#{i}"));
            (key, units.as_slice())
        })
        .collect()
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
            let name = name.clone();
            let removed = match old_unit {
                FrozenUnit::Struct { .. } => Some(BreakingChange::RemovedStruct { name }),
                FrozenUnit::Enum { .. } => Some(BreakingChange::RemovedEnum { name }),
                FrozenUnit::Protocol { .. } => Some(BreakingChange::RemovedProtocol { name }),
                FrozenUnit::Error { .. } => Some(BreakingChange::RemovedError { name }),
                FrozenUnit::Constant { .. } => Some(BreakingChange::RemovedConstant { name }),
                FrozenUnit::TypeAlias { .. } => Some(BreakingChange::RemovedTypeAlias { name }),
                FrozenUnit::Validator { .. } => Some(BreakingChange::RemovedValidator { name }),
                _ => None,
            };
            changes.breaking_changes.extend(removed);
        }
    }

    // Check for added declarations (features)
    for (name, new_unit) in &new_index {
        if !old_index.contains_key(name) {
            let name = name.clone();
            let added = match new_unit {
                FrozenUnit::Struct { fields, .. } => Some(NewFeature::AddedStruct {
                    name,
                    field_count: fields.len(),
                }),
                FrozenUnit::Enum { variants, .. } => Some(NewFeature::AddedEnum {
                    name,
                    variant_count: variants.len(),
                }),
                FrozenUnit::Protocol { functions, .. } => Some(NewFeature::AddedProtocol {
                    name,
                    function_count: functions.len(),
                }),
                FrozenUnit::Error { fields, .. } => Some(NewFeature::AddedError {
                    name,
                    field_count: fields.len(),
                }),
                FrozenUnit::Constant { ty, .. } => Some(NewFeature::AddedConstant {
                    name,
                    ty: ty.to_string(),
                }),
                FrozenUnit::TypeAlias { target, .. } => Some(NewFeature::AddedTypeAlias {
                    name,
                    target: target.to_string(),
                }),
                FrozenUnit::Validator { .. } => Some(NewFeature::AddedValidator { name }),
                _ => None,
            };
            changes.new_features.extend(added);
        }
    }

    // Error ordinal -> name, per side, so a function's `! Name` list (which
    // holds ordinals) is compared by name, not by numbers that shift.
    let old_errors = error_names(old_schema);
    let new_errors = error_names(new_schema);

    // Check for modifications to existing declarations
    for (name, new_unit) in &new_index {
        let Some(old_unit) = old_index.get(name) else {
            continue;
        };
        match (old_unit, new_unit) {
            (
                FrozenUnit::Struct {
                    docstring: old_docstring,
                    parameters: old_params,
                    fields: old_fields,
                    ..
                },
                FrozenUnit::Struct {
                    docstring: new_docstring,
                    parameters: new_params,
                    fields: new_fields,
                    ..
                },
            ) => {
                compare_struct_fields(name, old_fields, new_fields, &mut changes);
                compare_parameters(name, old_params, new_params, &mut changes);
                push_docstring_change(name, old_docstring, new_docstring, &mut changes);
            }
            (
                FrozenUnit::Enum {
                    docstring: old_docstring,
                    variants: old_variants,
                    ..
                },
                FrozenUnit::Enum {
                    docstring: new_docstring,
                    variants: new_variants,
                    ..
                },
            ) => {
                compare_enum_variants(name, old_variants, new_variants, &mut changes);
                push_docstring_change(name, old_docstring, new_docstring, &mut changes);
            }
            (
                FrozenUnit::Protocol {
                    docstring: old_docstring,
                    parameters: old_params,
                    functions: old_funcs,
                    ..
                },
                FrozenUnit::Protocol {
                    docstring: new_docstring,
                    parameters: new_params,
                    functions: new_funcs,
                    ..
                },
            ) => {
                compare_protocol_functions(
                    name,
                    (old_funcs, &old_errors),
                    (new_funcs, &new_errors),
                    &mut changes,
                );
                compare_parameters(name, old_params, new_params, &mut changes);
                // Protocol.docstring is a plain String (not Option<String>
                // like the others) - wrap for a uniform comparison helper.
                push_docstring_change(
                    name,
                    &Some(old_docstring.clone()),
                    &Some(new_docstring.clone()),
                    &mut changes,
                );
            }
            (
                FrozenUnit::Constant {
                    docstring: old_docstring,
                    ty: old_ty,
                    value: old_value,
                    ..
                },
                FrozenUnit::Constant {
                    docstring: new_docstring,
                    ty: new_ty,
                    value: new_value,
                    ..
                },
            ) => {
                if old_ty != new_ty {
                    changes
                        .breaking_changes
                        .push(BreakingChange::ChangedConstantType {
                            name: name.clone(),
                            old_type: old_ty.to_string(),
                            new_type: new_ty.to_string(),
                        });
                } else if old_value != new_value {
                    changes.new_features.push(NewFeature::ChangedConstantValue {
                        name: name.clone(),
                        old_value: old_value.as_ref().map(Literal::to_string),
                        new_value: new_value.as_ref().map(Literal::to_string),
                    });
                }
                push_docstring_change(name, old_docstring, new_docstring, &mut changes);
            }
            (
                FrozenUnit::Error {
                    docstring: old_docstring,
                    parameters: old_params,
                    ordinal: old_ordinal,
                    message: old_message,
                    fields: old_fields,
                    ..
                },
                FrozenUnit::Error {
                    docstring: new_docstring,
                    parameters: new_params,
                    ordinal: new_ordinal,
                    message: new_message,
                    fields: new_fields,
                    ..
                },
            ) => {
                if old_ordinal != new_ordinal {
                    changes
                        .breaking_changes
                        .push(BreakingChange::ChangedErrorOrdinal {
                            name: name.clone(),
                            old_ordinal: *old_ordinal,
                            new_ordinal: *new_ordinal,
                        });
                }
                // An error's fields travel like a struct's, so the same
                // rules apply.
                compare_struct_fields(name, old_fields, new_fields, &mut changes);
                if old_message != new_message {
                    changes
                        .modifications
                        .push(Modification::ChangedErrorMessage { name: name.clone() });
                }
                compare_parameters(name, old_params, new_params, &mut changes);
                push_docstring_change(name, old_docstring, new_docstring, &mut changes);
            }
            (
                FrozenUnit::TypeAlias {
                    docstring: old_docstring,
                    parameters: old_params,
                    target: old_target,
                    ..
                },
                FrozenUnit::TypeAlias {
                    docstring: new_docstring,
                    parameters: new_params,
                    target: new_target,
                    ..
                },
            ) => {
                if old_target != new_target {
                    changes
                        .breaking_changes
                        .push(BreakingChange::ChangedTypeAliasTarget {
                            name: name.clone(),
                            old_target: old_target.to_string(),
                            new_target: new_target.to_string(),
                        });
                }
                compare_parameters(name, old_params, new_params, &mut changes);
                push_docstring_change(name, old_docstring, new_docstring, &mut changes);
            }
            (
                FrozenUnit::Validator {
                    docstring: old_docstring,
                    ..
                },
                FrozenUnit::Validator {
                    docstring: new_docstring,
                    ..
                },
            ) => {
                if validator_shape(old_unit) != validator_shape(new_unit) {
                    changes
                        .breaking_changes
                        .push(BreakingChange::ChangedValidator { name: name.clone() });
                }
                push_docstring_change(name, old_docstring, new_docstring, &mut changes);
            }
            _ => {}
        }
    }

    changes
}

/// Record a `Modification::DocstringChanged` when a declaration's own
/// docstring differs between versions and nothing else about it is being
/// compared here (that's handled separately, e.g. `compare_struct_fields`
/// for a struct's fields) - a documentation-only change warrants a patch
/// bump, not silence.
fn push_docstring_change(
    name: &str,
    old_docstring: &Option<String>,
    new_docstring: &Option<String>,
    changes: &mut SchemaChanges,
) {
    if old_docstring != new_docstring {
        changes.modifications.push(Modification::DocstringChanged {
            name: name.to_string(),
        });
    }
}

fn build_index(schema: &[FrozenUnit]) -> HashMap<String, &FrozenUnit> {
    let mut index = HashMap::new();
    for unit in schema {
        let name = match unit {
            FrozenUnit::Struct { name, .. }
            | FrozenUnit::Enum { name, .. }
            | FrozenUnit::Protocol { name, .. }
            | FrozenUnit::Constant { name, .. }
            | FrozenUnit::Error { name, .. }
            | FrozenUnit::TypeAlias { name, .. }
            | FrozenUnit::Validator { name, .. } => Some(name.clone()),
            _ => None,
        };
        if let Some(n) = name {
            index.insert(n, unit);
        }
    }
    index
}

fn error_names(schema: &[FrozenUnit]) -> HashMap<u16, String> {
    schema
        .iter()
        .filter_map(|u| match u {
            FrozenUnit::Error { name, ordinal, .. } => Some((*ordinal, name.clone())),
            _ => None,
        })
        .collect()
}

struct FieldInfo<'a> {
    ty: &'a TypeRef,
    optional: bool,
    default: &'a Option<Literal>,
    parameters: &'a [FrozenUnit],
}

fn compare_struct_fields(
    struct_name: &str,
    old_fields: &[FrozenUnit],
    new_fields: &[FrozenUnit],
    changes: &mut SchemaChanges,
) {
    let old_field_map = build_field_map(old_fields);
    let new_field_map = build_field_map(new_fields);

    // Removed fields (breaking), optional ones included: the name is gone.
    for field_name in old_field_map.keys() {
        if !new_field_map.contains_key(field_name) {
            changes.breaking_changes.push(BreakingChange::RemovedField {
                type_name: struct_name.to_string(),
                field_name: field_name.clone(),
            });
        }
    }

    for (field_name, new) in &new_field_map {
        let Some(old) = old_field_map.get(field_name) else {
            // Added fields (feature if optional, breaking if required)
            if new.optional {
                changes.new_features.push(NewFeature::AddedField {
                    type_name: struct_name.to_string(),
                    field_name: field_name.clone(),
                    field_type: new.ty.to_string(),
                    optional: true,
                });
            } else {
                changes
                    .breaking_changes
                    .push(BreakingChange::AddedRequiredField {
                        type_name: struct_name.to_string(),
                        field_name: field_name.clone(),
                        field_type: new.ty.to_string(),
                    });
            }
            continue;
        };

        // Exact match, so `f32` -> `f64` and `u8[4]` -> `u8[8]` count.
        // Future: could allow compatible widening (e.g., u32 -> u64).
        if old.ty != new.ty {
            changes
                .breaking_changes
                .push(BreakingChange::ChangedFieldType {
                    type_name: struct_name.to_string(),
                    field_name: field_name.clone(),
                    old_type: old.ty.to_string(),
                    new_type: new.ty.to_string(),
                });
        }
        if old.optional != new.optional {
            changes
                .breaking_changes
                .push(BreakingChange::ChangedFieldOptionality {
                    type_name: struct_name.to_string(),
                    field_name: field_name.clone(),
                    now_optional: new.optional,
                });
        }
        if old.default != new.default {
            changes.modifications.push(Modification::ChangedFieldDefault {
                type_name: struct_name.to_string(),
                field_name: field_name.clone(),
            });
        }
        compare_parameters(
            &format!("{struct_name}.{field_name}"),
            old.parameters,
            new.parameters,
            changes,
        );
    }
}

fn build_field_map(fields: &[FrozenUnit]) -> HashMap<String, FieldInfo<'_>> {
    fields
        .iter()
        .filter_map(|field| match field {
            FrozenUnit::Field {
                name,
                ty,
                optional,
                default,
                parameters,
                ..
            } => Some((
                name.clone(),
                FieldInfo {
                    ty,
                    optional: *optional,
                    default,
                    parameters,
                },
            )),
            _ => None,
        })
        .collect()
}

fn compare_enum_variants(
    enum_name: &str,
    old_variants: &[FrozenUnit],
    new_variants: &[FrozenUnit],
    changes: &mut SchemaChanges,
) {
    let old_names = variant_names(old_variants);
    let new_names = variant_names(new_variants);
    let removed: Vec<&str> = old_names
        .iter()
        .copied()
        .filter(|v| !new_names.contains(v))
        .collect();
    let added: Vec<&str> = new_names
        .iter()
        .copied()
        .filter(|v| !old_names.contains(v))
        .collect();

    // Exactly one variant gone and one new in its place: a rename.
    if let ([old], [new]) = (removed.as_slice(), added.as_slice()) {
        let same_place = old_names.iter().position(|v| v == old)
            == new_names.iter().position(|v| v == new);
        if same_place {
            changes
                .breaking_changes
                .push(BreakingChange::RenamedEnumVariant {
                    enum_name: enum_name.to_string(),
                    old_name: old.to_string(),
                    new_name: new.to_string(),
                });
            return;
        }
    }

    // Removed variants (breaking)
    for variant in removed {
        changes
            .breaking_changes
            .push(BreakingChange::RemovedEnumVariant {
                enum_name: enum_name.to_string(),
                variant: variant.to_string(),
            });
    }

    // Added variants (feature)
    for variant in added {
        changes.new_features.push(NewFeature::AddedEnumVariant {
            enum_name: enum_name.to_string(),
            variant: variant.to_string(),
        });
    }
}

/// Variant names in declaration order.
fn variant_names(variants: &[FrozenUnit]) -> Vec<&str> {
    variants
        .iter()
        .filter_map(|v| match v {
            FrozenUnit::EnumVariant(name, _) => Some(name.as_str()),
            _ => None,
        })
        .collect()
}

struct FunctionInfo<'a> {
    signature: String,
    throws: Vec<String>,
    parameters: &'a [FrozenUnit],
}

fn compare_protocol_functions(
    protocol_name: &str,
    (old_funcs, old_errors): (&[FrozenUnit], &HashMap<u16, String>),
    (new_funcs, new_errors): (&[FrozenUnit], &HashMap<u16, String>),
    changes: &mut SchemaChanges,
) {
    let old_func_map = build_function_map(old_funcs, old_errors);
    let new_func_map = build_function_map(new_funcs, new_errors);

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

    for (func_name, new) in &new_func_map {
        let Some(old) = old_func_map.get(func_name) else {
            // Added functions (feature)
            changes.new_features.push(NewFeature::AddedFunction {
                protocol_name: protocol_name.to_string(),
                function_name: func_name.clone(),
                signature: new.signature.clone(),
            });
            continue;
        };

        // Changed function signatures (breaking)
        if old.signature != new.signature {
            changes
                .breaking_changes
                .push(BreakingChange::ChangedFunctionSignature {
                    protocol_name: protocol_name.to_string(),
                    function_name: func_name.clone(),
                    details: format!("{} → {}", old.signature, new.signature),
                });
        }

        for error in old.throws.iter().filter(|e| !new.throws.contains(e)) {
            changes
                .breaking_changes
                .push(BreakingChange::RemovedThrownError {
                    protocol_name: protocol_name.to_string(),
                    function_name: func_name.clone(),
                    error: error.clone(),
                });
        }
        for error in new.throws.iter().filter(|e| !old.throws.contains(e)) {
            changes.new_features.push(NewFeature::AddedThrownError {
                protocol_name: protocol_name.to_string(),
                function_name: func_name.clone(),
                error: error.clone(),
            });
        }

        compare_parameters(
            &format!("{protocol_name}.{func_name}"),
            old.parameters,
            new.parameters,
            changes,
        );
    }
}

fn build_function_map<'a>(
    functions: &'a [FrozenUnit],
    errors: &HashMap<u16, String>,
) -> HashMap<String, FunctionInfo<'a>> {
    functions
        .iter()
        .filter_map(|func| match func {
            FrozenUnit::Function {
                name,
                arguments,
                _return,
                throws,
                parameters,
                ..
            } => Some((
                name.clone(),
                FunctionInfo {
                    signature: format_function_signature(arguments, _return),
                    throws: throws
                        .iter()
                        .map(|o| errors.get(o).cloned().unwrap_or_else(|| format!("#{o}")))
                        .collect(),
                    parameters,
                },
            )),
            _ => None,
        })
        .collect()
}

/// Argument names are part of the signature: JSON keys parameters by name,
/// so renaming one breaks decoding there.
fn format_function_signature(args: &[FrozenArgument], ret: &Option<TypeRef>) -> String {
    let args: Vec<String> = args
        .iter()
        .map(|arg| format!("{}: {}", arg.name, arg.ty))
        .collect();

    let ret_type = ret
        .as_ref()
        .map(TypeRef::to_string)
        .unwrap_or_else(|| "void".to_string());

    format!("({}) -> {}", args.join(", "), ret_type)
}

/// A rendered `@handshake` value as the handshake it selects.
fn effective_handshake(rendered: &Option<String>) -> &str {
    rendered.as_deref().unwrap_or(crate::schema::idl::annotations::DEFAULT_HANDSHAKE)
}

/// Compare a declaration's annotations: `@key = value` properties,
/// `@deprecated`, and the validators applied to it.
fn compare_parameters(
    target: &str,
    old: &[FrozenUnit],
    new: &[FrozenUnit],
    changes: &mut SchemaChanges,
) {
    let old_props = property_map(old);
    let new_props = property_map(new);
    let keys: std::collections::BTreeSet<&str> =
        old_props.keys().chain(new_props.keys()).copied().collect();
    for key in keys {
        let old_value = old_props.get(key).map(|v| render_value(v));
        let new_value = new_props.get(key).map(|v| render_value(v));
        if old_value == new_value {
            continue;
        }
        // An absent `@handshake` means `comline@1`; spelling it out changes
        // nothing a peer sees.
        if key == "handshake" && effective_handshake(&old_value) == effective_handshake(&new_value) {
            continue;
        }
        match key {
            // Build-time only: it never reaches a peer or generated code.
            "settings" => {}
            // Adding or removing the marker is judged by the set it freezes,
            // below; only a reworded reason is reported here.
            "unverified" if old_value.is_none() || new_value.is_none() => {}
            "framing" | "handshake" => changes
                .breaking_changes
                .push(BreakingChange::ChangedWireAnnotation {
                    target: target.to_string(),
                    key: key.to_string(),
                    old_value,
                    new_value,
                }),
            _ => changes.modifications.push(Modification::ChangedAnnotation {
                target: target.to_string(),
                key: key.to_string(),
                old_value,
                new_value,
            }),
        }
    }

    let old_given_up = unverified::frozen_set(old);
    let new_given_up = unverified::frozen_set(new);
    let gave_up: Vec<Guarantee> =
        new_given_up.iter().filter(|g| !old_given_up.contains(g)).copied().collect();
    let restored: Vec<Guarantee> =
        old_given_up.iter().filter(|g| !new_given_up.contains(g)).copied().collect();
    if !gave_up.is_empty() {
        changes.breaking_changes.push(BreakingChange::GaveUpGuarantees {
            target: target.to_string(),
            guarantees: gave_up,
        });
    }
    if !restored.is_empty() {
        changes.new_features.push(NewFeature::RestoredGuarantees {
            target: target.to_string(),
            guarantees: restored,
        });
    }

    let old_deprecated = deprecation(old);
    let new_deprecated = deprecation(new);
    if old_deprecated != new_deprecated {
        if old_deprecated.is_none() {
            changes.new_features.push(NewFeature::Deprecated {
                target: target.to_string(),
            });
        } else {
            changes.modifications.push(Modification::ChangedAnnotation {
                target: target.to_string(),
                key: "deprecated".to_string(),
                old_value: old_deprecated,
                new_value: new_deprecated,
            });
        }
    }

    let old_validators = validator_refs(old);
    let new_validators = validator_refs(new);
    if old_validators != new_validators {
        let only_removed = new_validators.len() < old_validators.len()
            && new_validators.iter().all(|v| old_validators.contains(v));
        if only_removed {
            changes.new_features.push(NewFeature::ValidationLoosened {
                target: target.to_string(),
            });
        } else {
            changes
                .breaking_changes
                .push(BreakingChange::ValidationTightened {
                    target: target.to_string(),
                });
        }
    }
}

fn property_map(parameters: &[FrozenUnit]) -> BTreeMap<&str, &ConfigValue> {
    parameters
        .iter()
        .filter_map(|p| match p {
            FrozenUnit::Property { name, value } => Some((name.as_str(), value)),
            _ => None,
        })
        .collect()
}

fn render_value(value: &ConfigValue) -> String {
    value
        .as_text()
        .map(|t| t.into_owned())
        .unwrap_or_else(|| "(marker)".to_string())
}

/// `@deprecated`'s message, or `""` for a bare one. `None` when it's absent
/// or `false`.
fn deprecation(parameters: &[FrozenUnit]) -> Option<String> {
    parameters.iter().find_map(|p| match p {
        FrozenUnit::Deprecated(DeprecatedValue::False) => None,
        FrozenUnit::Deprecated(DeprecatedValue::Message(m)) => Some(m.clone()),
        FrozenUnit::Deprecated(other) => Some(format!("{other:?}")),
        _ => None,
    })
}

/// The validators applied to a declaration, without source positions, so
/// moving them around in the file isn't a change.
fn validator_refs(parameters: &[FrozenUnit]) -> Vec<FrozenUnit> {
    parameters
        .iter()
        .filter(|p| matches!(p, FrozenUnit::ValidatorRef { .. }))
        .map(without_spans)
        .collect()
}

/// A validator declaration without its docstring or source positions: what
/// decides whether it accepts something differently.
fn validator_shape(unit: &FrozenUnit) -> Option<FrozenUnit> {
    match without_spans(unit) {
        FrozenUnit::Validator {
            properties,
            name,
            targets,
            expression_block,
            ..
        } => Some(FrozenUnit::Validator {
            docstring: None,
            properties,
            name,
            targets,
            expression_block,
            span: (0, 0),
            validate_span: None,
        }),
        _ => None,
    }
}

/// A copy of `unit` with every source position cleared, for comparing
/// content regardless of formatting. Covers the units validators and their
/// references are built from.
fn without_spans(unit: &FrozenUnit) -> FrozenUnit {
    let all = |units: &[FrozenUnit]| units.iter().map(without_spans).collect::<Vec<_>>();
    match unit {
        FrozenUnit::Validator {
            docstring,
            properties,
            name,
            targets,
            expression_block,
            ..
        } => FrozenUnit::Validator {
            docstring: docstring.clone(),
            properties: all(properties),
            name: name.clone(),
            targets: targets.clone(),
            expression_block: Box::new(without_spans(expression_block)),
            span: (0, 0),
            validate_span: None,
        },
        FrozenUnit::ValidatorRef { name, args, .. } => FrozenUnit::ValidatorRef {
            name: name.clone(),
            args: all(args),
            span: (0, 0),
            arg_spans: vec![],
        },
        FrozenUnit::ExpressionBlock { asserts } => FrozenUnit::ExpressionBlock {
            asserts: all(asserts),
        },
        FrozenUnit::Assert {
            condition,
            message,
            references,
            expr,
            uninterpolated,
            ..
        } => FrozenUnit::Assert {
            condition: condition.clone(),
            message: message.clone(),
            references: references.clone(),
            expr: expr.clone(),
            message_span: None,
            uninterpolated: uninterpolated.clone(),
        },
        FrozenUnit::Field {
            docstring,
            parameters,
            optional,
            name,
            ty,
            default,
            ..
        } => FrozenUnit::Field {
            docstring: docstring.clone(),
            parameters: all(parameters),
            optional: *optional,
            name: name.clone(),
            ty: ty.clone(),
            default: default.clone(),
            span: (0, 0),
        },
        other => other.clone(),
    }
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
        left_value: Option<&Literal>,
        right_value: Option<&Literal>,
    );
}
