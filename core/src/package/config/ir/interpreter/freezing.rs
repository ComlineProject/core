// Standard Uses

// Crate Uses
use crate::package::config::idl::grammar::{Assignment, BooleanLiteral, Key, Value};
use crate::package::config::ir::context::ProjectContext;
use crate::package::config::ir::frozen::{
    Dependency, FrozenUnit, LanguageDetails, PublishRegistry, RegistryKind,
};
// use crate::utils::codemap::Span;

// External Uses

use crate::schema::ir::compiler::settings::conformance::check_settings_conformance;

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
                return Err("'specification_version' should be a number".into());
            };

            let version_num: u8 = version.value.parse().map_err(|_| {
                format!("'specification_version' is not a valid version number: {}", version.value)
            })?;
            vec![FrozenUnit::SpecificationVersion(version_num)]
        }
        /*
        "schema_paths" => {
            let Value::List(paths) = &node.value else {
                return Err("'schema_paths' should be a list of paths".into())
            };

            let mut solved = vec![];
            for path_val in &paths.items {
                let Value::String(path) = path_val else {
                    return Err("'schema_paths' entries should be path strings".into())
                };

                let schema_file = context.find_schema_by_filename(&path.value);

                if schema_file.is_none() {
                    return Err(format!("no schema found with the path '{}'", path.value).into());
                }

                solved.push(FrozenUnit::SchemaPath(path.value.clone()));
            }

            solved
        },
        */
        "code_generation" => {
            let Value::Dictionary(items) = &node.value else {
                return Err("'code_generation' should be a dictionary".into())
            };

            interpret_assignment_code_generation(&items.assignments)?
        }
        "publish_registries" => {
            let Value::Dictionary(items) = &node.value else {
                return Err("'publish_registries' should be a dictionary".into())
            };

            interpret_assigment_publish_registries(&items.assignments)?
        }
        "dependencies" => {
            let Value::Dictionary(items) = &node.value else {
                return Err("'dependencies' should be a dictionary".into())
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
                        // The same registry check the `.ids` path runs, so
                        // an unrecognised key or wrongly-typed value in a
                        // package manifest is caught too - it subsumes the
                        // `mode`-only check that used to live here.
                        //
                        // `.idp` settings have no per-entry span tracking
                        // yet, so these carry no span rather than a wrong
                        // one. Returned as an error instead of panicking,
                        // unlike this function's other arms.
                        if let Err(diagnostics) = check_settings_conformance(
                            &dict,
                            &crate::settings::SettingsSpans::new(),
                            "the package's 'settings'",
                            None,
                        ) {
                            let joined = diagnostics
                                .iter()
                                .map(|d| format!("[{}] {}", d.code.id, d.message))
                                .collect::<Vec<_>>()
                                .join("; ");
                            return Err(format!("invalid 'settings': {joined}").into());
                        }
                        vec![FrozenUnit::Settings(dict)]
                    }
                    Value::Namespaced(ns) => {
                        return Err(format!(
                            "settings = {} is a cross-package reference, which needs a \
                             filesystem build with dependency resolution — not available in \
                             this context",
                            ns.value
                        )
                        .into())
                    }
                    _ => return Err("'settings' should be a dictionary".into()),
                }
            }
        }
        any => return Err(format!("'{any}' is not a recognised package-manifest key").into()),
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
                    return Err("'code_generation.languages' should be a dictionary".into())
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
                        return Err(format!(
                            "'code_generation.languages.{lang_name}' should be a dictionary"
                        )
                        .into())
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
                        return Err(format!(
                            "`code_generation.languages.{lang_name}` takes no options \
                             (got `{detail_key}`); write `{lang_name} = {{}}`"
                        )
                        .into());
                    }

                    languages.push(FrozenUnit::CodeGeneration(LanguageDetails {
                        name: lang_name,
                    }));
                }
            }
            other => {
                return Err(format!("'{other}' is not allowed in 'code_generation'").into())
            }
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
                                return Err("'uri' should be a string".into())
                            }
                        }
                        // method...
                        other => {
                            return Err(format!(
                                "'{other}' is not allowed in 'publish_registries'"
                            )
                            .into())
                        }
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
            other => {
                return Err(format!(
                    "a publish registry should be a dictionary, got {other:?}"
                )
                .into())
            }
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

    let deps = DependencyConfig::parse_dict(dict).map_err(|msg| -> Box<dyn snafu::Error> {
        msg.into()
    })?;

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
        Value::Boolean(BooleanLiteral::True) => SettingsValue::Bool(true),
        Value::Boolean(BooleanLiteral::False) => SettingsValue::Bool(false),
        Value::Number(n) => SettingsValue::Integer(
            n.value
                .parse()
                .map_err(|_| format!("'{}' is not a valid settings integer", n.value))?,
        ),
        Value::String(s) => SettingsValue::Str(strip_quotes(&s.value)),
        // A reserved bare keyword (`replace`) - `true`/`false` are now
        // `Boolean`'s literal-text leaves, never `Identifier`'s regex, so
        // this arm no longer needs (or has) a `true`/`false` special case.
        Value::Identifier(id) => SettingsValue::Identifier(id.value.clone()),
        Value::Dictionary(d) => SettingsValue::Dict(interpret_settings_dict(d)?),
        other => {
            return Err(format!(
                "a settings value must be a boolean, number, string, bare keyword, or \
                 nested dictionary; got {other:?}"
            )
            .into())
        }
    })
}
