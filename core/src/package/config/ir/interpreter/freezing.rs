// Standard Uses

// Crate Uses
use crate::package::config::idl::grammar::{Assignment, Key, Value};
use crate::package::config::ir::context::ProjectContext;
use crate::package::config::ir::frozen::{
    Dependency, FrozenUnit, FrozenWhole, LanguageDetails, PublishRegistry, RegistryKind,
};
// use crate::utils::codemap::Span;

// External Uses

#[allow(unused)]
pub fn interpret_node_into_frozen(
    context: &ProjectContext,
    node: &Assignment,
    cross_package_settings: Option<&crate::settings::SettingsDict>,
) -> Result<Vec<FrozenUnit>, Box<dyn snafu::Error>> {
    interpret_assignment(context, node, cross_package_settings)
}

pub fn interpret_assignment(
    _context: &ProjectContext,
    node: &Assignment,
    cross_package_settings: Option<&crate::settings::SettingsDict>,
) -> Result<Vec<FrozenUnit>, Box<dyn snafu::Error>> {
    let key_str = match &node.key {
        Key::Identifier(id) => id.value.clone(),
        Key::Namespaced(ns) => ns.value.clone(),
        Key::VersionMeta(vm) => vm.value.clone(),
        Key::DependencyAddress(da) => da.value.clone(),
    };

    let result = match key_str.as_str() {
        "specification_version" => {
            let Value::Number(version) = &node.value else {
                panic!(
                    "'specification_version' should be a number, \
                    got something else instead."
                )
            };

            // Should parse integer
            let version_num: u8 = version.value.parse().expect("Invalid version number");
            vec![FrozenUnit::SpecificationVersion(version_num)]
        }
        /*
        "schemas_source_path" => {
            todo!()
        }
        */
        /*
        "schema_paths" => {
            let Value::List(paths) = &node.value else {
                panic!("'schema_paths' should be a list of paths")
            };

            let mut solved = vec![];
            for path_val in &paths.items {
                let Value::String(path) = path_val else {
                    panic!("Expected path string")
                };

                let schema_file = context.find_schema_by_filename(&path.value);

                if schema_file.is_none() { panic!("No schema found with the path: '{}'", path.value) }

                solved.push(FrozenUnit::SchemaPath(path.value.clone()));
            }

            solved
        },
        */
        "code_generation" => {
            let Value::Dictionary(items) = &node.value else {
                panic!("Expected dictionary for code_generation")
            };

            interpret_assignment_code_generation(&items.assignments)?
        }
        "publish_registries" => {
            let Value::Dictionary(items) = &node.value else {
                panic!("Expected dictionary for publish_registries")
            };

            interpret_assigment_publish_registries(&items.assignments)?
        }
        "dependencies" => {
            let Value::Dictionary(items) = &node.value else {
                panic!("Expected dictionary for dependencies")
            };

            interpret_assignment_dependencies(items)?
        }
        "settings" => {
            if let Some(resolved) = cross_package_settings {
                vec![FrozenUnit::Settings(resolved.clone())]
            } else {
                match &node.value {
                    Value::Dictionary(items) => {
                        let dict = interpret_settings_dict(items)?;
                        if let Err(errors) = crate::settings::merge::validate_mode_keys(&dict) {
                            panic!(
                                "invalid 'settings': {}",
                                errors
                                    .iter()
                                    .map(|e| format!(
                                        "'mode' at '{}' is {:?}, not the bare keyword `replace`",
                                        e.path, e.found
                                    ))
                                    .collect::<Vec<_>>()
                                    .join("; ")
                            );
                        }
                        vec![FrozenUnit::Settings(dict)]
                    }
                    Value::Namespaced(ns) => panic!(
                        "settings = {} is a cross-package reference, which needs a filesystem \
                         build with dependency resolution — not available in this context",
                        ns.value
                    ),
                    _ => panic!("'settings' should be a dictionary"),
                }
            }
        }
        any => {
            // panic!("Assignment '{}' is not a valid assignment", any)
            // Allow unknown assignments for now or warn?
            // panic for now to match behavior
            panic!("Assignment '{}' is not a valid assignment", any)
        }
    };

    Ok(result)
}

fn interpret_assignment_code_generation(
    items: &[rust_sitter::Spanned<Assignment>],
) -> Result<Vec<FrozenUnit>, Box<dyn snafu::Error>> {
    let mut languages = vec![];

    for assignment in items {
        let assignment = &assignment.value;
        let key_str = match &assignment.key {
            Key::Identifier(id) => id.value.clone(),
            Key::Namespaced(ns) => ns.value.clone(),
            Key::VersionMeta(vm) => vm.value.clone(),
            Key::DependencyAddress(da) => da.value.clone(),
        };

        match key_str.as_str() {
            "languages" => {
                // Value should be Dictionary of Language -> Details
                let Value::Dictionary(lang_dict) = &assignment.value else {
                    panic!("languages must be a dictionary")
                };

                for lang_assign in &lang_dict.assignments {
                    let lang_assign = &lang_assign.value;
                    let lang_name = match &lang_assign.key {
                        Key::Identifier(id) => id.value.clone(),
                        Key::Namespaced(ns) => ns.value.clone(),
                        Key::VersionMeta(vm) => vm.value.clone(),
                        Key::DependencyAddress(da) => da.value.clone(),
                    };

                    let Value::Dictionary(details) = &lang_assign.value else {
                        panic!("Language details must be a dictionary")
                    };

                    // A declared language takes no options, it is just a
                    // capability declaration. Output location and which package
                    // versions to generate are consumer-side config, not the
                    // congregation. Write `{} = {{}}`.
                    if let Some(detail) = details.assignments.first() {
                        let detail_key = match &detail.key {
                            Key::Identifier(id) => id.value.as_str(),
                            Key::Namespaced(ns) => ns.value.as_str(),
                            Key::VersionMeta(vm) => vm.value.as_str(),
                            Key::DependencyAddress(da) => da.value.as_str(),
                        };
                        panic!(
                            "`code_generation.languages.{lang_name}` takes no options \
                             (got `{detail_key}`); write `{lang_name} = {{}}`"
                        );
                    }

                    languages.push(FrozenUnit::CodeGeneration(LanguageDetails {
                        name: lang_name,
                    }));
                }
            }
            other => panic!("Key not allowed here: {}", other),
        }
    }

    Ok(languages)
}

fn interpret_assigment_publish_registries(
    items: &[rust_sitter::Spanned<Assignment>],
) -> Result<Vec<FrozenUnit>, Box<dyn snafu::Error>> {
    let mut targets = vec![];

    for assignment in items {
        let assignment = &assignment.value;
        let key_str = match &assignment.key {
            Key::Identifier(id) => id.value.clone(),
            Key::Namespaced(ns) => ns.value.clone(),
            Key::VersionMeta(vm) => vm.value.clone(),
            Key::DependencyAddress(da) => da.value.clone(),
        };

        let target = match &assignment.value {
            Value::String(_name) => FrozenUnit::PublishRegistry((
                key_str,
                PublishRegistry {
                    kind: RegistryKind::LocalStorage,
                    uri: "none".to_string(),
                },
            )),
            Value::Identifier(_name) => {
                FrozenUnit::PublishRegistry((
                    key_str,
                    PublishRegistry {
                        kind: RegistryKind::LocalStorage, // TODO: logic for identifier registry?
                        uri: "none".to_string(),
                    },
                ))
            }
            Value::Namespaced(_ns) => {
                FrozenUnit::PublishRegistry((
                    key_str,
                    PublishRegistry {
                        kind: RegistryKind::LocalStorage, // TODO: resolve namespaced registry
                        uri: "none".to_string(),
                    },
                ))
            }
            Value::Dictionary(dict) => {
                let mut url = None;
                let mut registry_kind = None;

                for item in &dict.assignments {
                    let item = &item.value;
                    let item_key = match &item.key {
                        Key::Identifier(id) => id.value.clone(),
                        Key::Namespaced(ns) => ns.value.clone(),
                        Key::VersionMeta(vm) => vm.value.clone(),
                        Key::DependencyAddress(da) => da.value.clone(),
                    };

                    match item_key.as_str() {
                        "uri" => {
                            if let Value::String(s) = &item.value {
                                registry_kind = Some(RegistryKind::LocalStorage);
                                url = Some(s.value.clone());
                            } else {
                                panic!("URI should be a string")
                            }
                        }
                        // method...
                        other => panic!("Key not allowed here: {}", other),
                    }
                }

                FrozenUnit::PublishRegistry((
                    key_str,
                    PublishRegistry {
                        kind: registry_kind.unwrap(),
                        uri: url.unwrap(),
                    },
                ))
            }
            other => panic!("Invalid registry value: {:?}", other),
        };

        targets.push(target);
    }

    Ok(targets)
}

/// `dependencies = { name = { version, uri/path/commit, hash }, ... }` — each
/// entry freezes to a `FrozenUnit::Dependency`. A `Path` dependency has no
/// declared version (there's nothing to read without touching the
/// filesystem, which this interpretation pass deliberately never does); it
/// freezes with a placeholder `"unresolved"` version that
/// `package::deps::resolve` overwrites with the dependency's actual current
/// version once it resolves it (see `compile_package`).
fn interpret_assignment_dependencies(
    dict: &crate::package::config::idl::grammar::Dictionary,
) -> Result<Vec<FrozenUnit>, Box<dyn snafu::Error>> {
    use crate::package::config::dependency::DependencyConfig;

    let deps = match DependencyConfig::parse_dict(dict) {
        Ok(deps) => deps,
        Err(msg) => panic!("{msg}"),
    };

    let mut names: Vec<&String> = deps.keys().collect();
    names.sort(); // deterministic freeze order regardless of HashMap iteration

    Ok(names
        .into_iter()
        .map(|name| {
            let dep = &deps[name];
            FrozenUnit::Dependency(Dependency {
                author: dep.author(),
                project: dep.name.clone(),
                version: dep.declared_version().unwrap_or("unresolved").to_string(),
            })
        })
        .collect())
}

/// `settings = { ... }` - package-wide authoring policy, applying to
/// every schema by default. Unlike `code_generation`/`publish_registries`,
/// this dictionary's own nesting is meaningful content (not just grammar
/// structure) - converted recursively into `crate::settings::SettingsDict`,
/// the same shared tree `.ids`'s dotted-key settings desugar into.
fn interpret_settings_dict(
    dict: &crate::package::config::idl::grammar::Dictionary,
) -> Result<crate::settings::SettingsDict, Box<dyn snafu::Error>> {
    use crate::settings::value::SettingsDict;

    let mut out = SettingsDict::new();
    for assignment in &dict.assignments {
        let assignment = &assignment.value;
        let key_str = match &assignment.key {
            Key::Identifier(id) => id.value.clone(),
            Key::Namespaced(ns) => ns.value.clone(),
            Key::VersionMeta(vm) => vm.value.clone(),
            Key::DependencyAddress(da) => da.value.clone(),
        };
        out.0.insert(key_str, interpret_settings_value(&assignment.value)?);
    }
    Ok(out)
}

fn interpret_settings_value(
    value: &Value,
) -> Result<crate::settings::SettingsValue, Box<dyn snafu::Error>> {
    use crate::package::config::dependency::strip_quotes;
    use crate::settings::value::SettingsValue;

    Ok(match value {
        Value::Boolean(b) => SettingsValue::Bool(b.value == "true"),
        Value::Number(n) => SettingsValue::Integer(
            n.value
                .parse()
                .unwrap_or_else(|_| panic!("'{}' is not a valid settings integer", n.value)),
        ),
        Value::String(s) => SettingsValue::Str(strip_quotes(&s.value)),
        // `true`/`false` and a reserved bare keyword (`replace`) are both
        // lexically `[a-zA-Z_][a-zA-Z0-9_]*` - `Identifier`'s pattern,
        // not `Boolean`'s, is what actually matches them here (confirmed:
        // `grep`ping this grammar's own tests, nothing ever asserts which
        // `Value` variant bare `true`/`false` produces). So both cases are
        // handled on this one arm, not two.
        Value::Identifier(id) if id.value == "true" => SettingsValue::Bool(true),
        Value::Identifier(id) if id.value == "false" => SettingsValue::Bool(false),
        Value::Identifier(id) => SettingsValue::Identifier(id.value.clone()),
        Value::Dictionary(d) => SettingsValue::Dict(interpret_settings_dict(d)?),
        other => panic!(
            "a settings value must be a boolean, number, string, bare keyword, or nested \
             dictionary; got {:?}",
            other
        ),
    })
}

#[allow(unused)]
pub fn into_frozen_whole(
    context: &ProjectContext,
    interpreted: Vec<FrozenUnit>,
) -> Result<FrozenWhole, Box<dyn snafu::Error>> {
    todo!()
}
