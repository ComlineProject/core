// Standard Uses

// Crate Uses
use crate::package::config::idl::grammar::{Assignment, BooleanLiteral, Value};
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
    context: &ProjectContext,
    node: &Assignment,
    cross_package_settings: Option<&crate::settings::SettingsDict>,
) -> Result<Vec<FrozenUnit>, Box<dyn snafu::Error>> {
    let key_str = node.key.text();

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
        "handshake" => {
            let Value::String(name) = &node.value else {
                return Err("'handshake' should be a string, like \"comline@1\"".into())
            };
            let written = crate::package::config::dependency::strip_quotes(&name.value);
            let name = crate::schema::idl::annotations::check_handshake_or_namespaced(&written)?;
            vec![FrozenUnit::Handshake(name)]
        }
        "wire" => {
            let Value::Dictionary(items) = &node.value else {
                return Err("'wire' should be a dictionary, like \
                            { format = \"packed@1\", framing = \"datagram@2\" }"
                    .into());
            };
            interpret_assignment_wire(&items.assignments)?
        }
        "encodings" => {
            let Value::Dictionary(items) = &node.value else {
                return Err("'encodings' should be a dictionary, like \
                            { \"com.acme.x@1\" = { kind = \"format\", profile = \"profiles/x.json\" } }"
                    .into());
            };
            interpret_assignment_encodings(context, &items.assignments)?
        }
        any => return Err(format!("'{any}' is not a recognised package-manifest key").into()),
    };

    Ok(result)
}

/// `wire = { format = "...", framing = "..." }`: both entries optional,
/// each folded onto its canonical name.
fn interpret_assignment_wire(
    items: &[rust_sitter::Spanned<Assignment>],
) -> Result<Vec<FrozenUnit>, Box<dyn snafu::Error>> {
    let (mut format, mut framing) = (None, None);
    for assignment in items {
        let assignment = &assignment.value;
        let key = assignment.key.text();
        let Value::String(written) = &assignment.value else {
            return Err(format!("'wire.{key}' should be a string, like \"packed@1\"").into());
        };
        let written = crate::package::config::dependency::strip_quotes(&written.value);
        // A namespaced name is checked once dependencies are known, since
        // one of them may declare it (`compiler::interpret`).
        let (slot, name) = match key.as_str() {
            "format" => (&mut format, crate::schema::idl::annotations::check_format_or_namespaced(&written)?),
            "framing" => (&mut framing, crate::schema::idl::annotations::check_framing_or_namespaced(&written)?),
            other => {
                return Err(format!("'{other}' is not allowed in 'wire': expected `format` or `framing`").into())
            }
        };
        if slot.replace(name.to_string()).is_some() {
            return Err(format!("'wire.{key}' is set twice").into());
        }
    }
    Ok(vec![FrozenUnit::Wire { format, framing }])
}

/// `encodings = { "<name>@<n>" = { kind, profile, map }, ... }`: each entry
/// freezes to a `FrozenUnit::Encoding`, its profile read from the package
/// directory and checked. Errors point at the entry in `config.idp`.
fn interpret_assignment_encodings(
    context: &ProjectContext,
    items: &[rust_sitter::Spanned<Assignment>],
) -> Result<Vec<FrozenUnit>, Box<dyn snafu::Error>> {
    use crate::diagnostics::Diagnostic;
    use crate::encodings::{self, Profile, ProfileKind};
    use crate::package::config::dependency::strip_quotes;
    use crate::package::config::ir::context::Origin;

    let config_path = match &context.origin {
        Origin::Disk(path) => Some(path.as_path()),
        Origin::Virtual | Origin::Official => None,
    };
    let fail = |code, message: String, span: (usize, usize)| -> Box<dyn snafu::Error> {
        let diagnostic = Diagnostic::new(code, message).with_span(span);
        let source = config_path.and_then(|p| std::fs::read_to_string(p).ok());
        match source {
            Some(source) => crate::diagnostics::render(&diagnostic, "config.idp", &source).into(),
            None => diagnostic.message.into(),
        }
    };
    let policy = encodings::name_policy(context);

    let mut units = vec![];
    let mut seen = std::collections::HashSet::new();
    for entry in items {
        let name = entry.value.key.text();
        // The key's own span: the entry starts with it.
        let key_len = match &entry.value.key {
            crate::package::config::idl::grammar::Key::String(s) => s.value.len(),
            _ => name.len(),
        };
        let span = (entry.span.0, (entry.span.0 + key_len).min(entry.span.1));
        encodings::check_declared_name(&name, policy)
            .map_err(|m| fail(encodings::ENCODING_NAME, m, span))?;
        if !seen.insert(name.clone()) {
            return Err(fail(encodings::ENCODING_NAME, format!("encoding `{name}` is declared twice"), span));
        }
        let Value::Dictionary(fields) = &entry.value.value else {
            return Err(fail(
                encodings::ENCODING_ENTRY,
                format!("encoding `{name}` should be a dictionary, like {{ kind = \"format\", profile = \"profiles/x.json\" }}"),
                span,
            ));
        };

        let (mut kind, mut profile, mut map) = (None, None, encodings::EncodingMap::new());
        for field in &fields.assignments {
            let key = field.value.key.text();
            match (key.as_str(), &field.value.value) {
                ("kind", Value::String(k)) => {
                    kind = Some(match strip_quotes(&k.value).as_str() {
                        "format" => ProfileKind::Format,
                        "framing" => ProfileKind::Framing,
                        "handshake" => ProfileKind::Handshake,
                        other => {
                            return Err(fail(
                                encodings::ENCODING_ENTRY,
                                format!("encoding `{name}`: kind `{other}` isn't one: expected `format`, `framing` or `handshake`"),
                                span,
                            ))
                        }
                    })
                }
                ("profile", Value::String(p)) => profile = Some(strip_quotes(&p.value)),
                ("map", Value::Dictionary(languages)) => {
                    for language in &languages.assignments {
                        let id = language.value.key.text();
                        let mapping = interpret_encoding_mapping(&language.value.value)
                            .map_err(|m| fail(encodings::ENCODING_ENTRY, format!("encoding `{name}`, `map.{id}`: {m}"), span))?;
                        map.insert(id, mapping);
                    }
                }
                ("kind" | "profile", _) => {
                    return Err(fail(encodings::ENCODING_ENTRY, format!("encoding `{name}`: `{key}` should be a string"), span))
                }
                ("map", _) => {
                    return Err(fail(
                        encodings::ENCODING_ENTRY,
                        format!("encoding `{name}`: `map` should be a dictionary, like {{ rust = {{ type = \"...\" }} }}"),
                        span,
                    ))
                }
                (other, _) => {
                    return Err(fail(
                        encodings::ENCODING_ENTRY,
                        format!("encoding `{name}`: `{other}` isn't allowed: expected `kind`, `profile` or `map`"),
                        span,
                    ))
                }
            }
        }
        let kind = kind.ok_or_else(|| fail(encodings::ENCODING_ENTRY, format!("encoding `{name}` needs a `kind`"), span))?;
        let profile = profile.ok_or_else(|| fail(encodings::ENCODING_PROFILE, format!("encoding `{name}` needs a `profile`"), span))?;

        // The profile, relative to the package root: a directory, or the
        // official package's embedded files.
        let root = match (&context.origin, config_path.and_then(|p| p.parent())) {
            (Origin::Official, _) => Some(ProfileRoot::Official),
            (_, Some(dir)) => Some(ProfileRoot::Dir(dir)),
            (_, None) => None,
        };
        let Some(root) = root else {
            return Err(fail(
                encodings::ENCODING_PROFILE,
                format!("encoding `{name}`: its profile `{profile}` can't be read without a package directory"),
                span,
            ));
        };
        let profile_path = std::path::Path::new(&profile);
        if profile_path.is_absolute() || profile_path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            return Err(fail(
                encodings::ENCODING_PROFILE,
                format!("encoding `{name}`: its profile `{profile}` must be a path inside the package, relative to it"),
                span,
            ));
        }
        let bytes = root.read(profile_path).map_err(|e| {
            fail(encodings::ENCODING_PROFILE, format!("encoding `{name}`: its profile `{profile}` can't be read: {e}"), span)
        })?;
        let parsed = Profile::parse(&bytes).map_err(|m| {
            fail(encodings::ENCODING_PROFILE, format!("encoding `{name}`: its profile `{profile}` is malformed: {m}"), span)
        })?;
        if parsed.kind != kind {
            return Err(fail(
                encodings::ENCODING_PROFILE,
                format!(
                    "encoding `{name}` is a {}, but its profile `{profile}` is a {} profile",
                    kind.as_str(),
                    parsed.kind.as_str()
                ),
                span,
            ));
        }
        if parsed.name != name {
            return Err(fail(
                encodings::ENCODING_PROFILE,
                format!("encoding `{name}`: its profile `{profile}` is for `{}`", parsed.name),
                span,
            ));
        }

        // Conformance vectors sit beside the profile: `profiles/x.json`'s
        // are `profiles/x.vectors.json`. Official encodings always have them
        // (the runtime's corpus).
        let has_vectors = match root {
            ProfileRoot::Official => true,
            ProfileRoot::Dir(dir) => dir.join(vectors_path(profile_path)).is_file(),
        };
        units.push(FrozenUnit::Encoding {
            name,
            kind,
            profile_hash: blake3::hash(&bytes).to_hex().to_string(),
            map,
            establishes: parsed.establishes(),
            has_vectors,
        });
    }
    Ok(units)
}

/// Where an encoding's conformance vectors sit, by convention: beside its
/// profile, `profiles/x.json` → `profiles/x.vectors.json`.
pub fn vectors_path(profile: &std::path::Path) -> std::path::PathBuf {
    profile.with_extension("vectors.json")
}

/// Where a package's profile paths resolve: its directory, or the official
/// package's embedded files.
enum ProfileRoot<'a> {
    Dir(&'a std::path::Path),
    Official,
}

impl ProfileRoot<'_> {
    fn read(&self, path: &std::path::Path) -> std::io::Result<Vec<u8>> {
        match self {
            ProfileRoot::Dir(dir) => std::fs::read(dir.join(path)),
            ProfileRoot::Official => comline_core_stdlib::OFFICIAL_PROFILES
                .iter()
                .find(|(embedded, _)| std::path::Path::new(embedded) == path)
                .map(|(_, contents)| contents.as_bytes().to_vec())
                .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound)),
        }
    }
}

/// One language's `{ type = "...", package = "...", version = "..." }`.
fn interpret_encoding_mapping(
    value: &Value,
) -> Result<crate::encodings::EncodingMapping, String> {
    use crate::package::config::dependency::strip_quotes;

    let Value::Dictionary(fields) = value else {
        return Err("should be a dictionary, like { type = \"...\" }".to_string());
    };
    let (mut ty, mut package, mut version) = (None, None, None);
    for field in &fields.assignments {
        let key = field.value.key.text();
        let Value::String(text) = &field.value.value else {
            return Err(format!("`{key}` should be a string"));
        };
        let slot = match key.as_str() {
            "type" => &mut ty,
            "package" => &mut package,
            "version" => &mut version,
            other => return Err(format!("`{other}` isn't allowed: expected `type`, `package` or `version`")),
        };
        if slot.replace(strip_quotes(&text.value)).is_some() {
            return Err(format!("`{key}` is set twice"));
        }
    }
    Ok(crate::encodings::EncodingMapping {
        ty: ty.ok_or("needs a `type`")?,
        package,
        version,
    })
}

fn interpret_assignment_code_generation(
    items: &[rust_sitter::Spanned<Assignment>],
) -> Result<Vec<FrozenUnit>, Box<dyn snafu::Error>> {
    let mut languages = vec![];

    for assignment in items {
        let assignment = &assignment.value;
        let key_str = assignment.key.text();

        match key_str.as_str() {
            "languages" => {
                // Value should be Dictionary of Language -> Details
                let Value::Dictionary(lang_dict) = &assignment.value else {
                    return Err("'code_generation.languages' should be a dictionary".into())
                };

                for lang_assign in &lang_dict.assignments {
                    let lang_assign = &lang_assign.value;
                    let lang_name = lang_assign.key.text();

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
                        let detail_key = detail.key.text();
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
        let key_str = assignment.key.text();

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
                    let item_key = item.key.text();

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
        let key_str = assignment.key.text();
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
