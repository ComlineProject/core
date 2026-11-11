//! The specification's compiler conformance cases (`compiler/cases/`, vendored
//! in `tests/spec/cases`, core#224): each is a schema graph as data with the
//! outcome any compiler must reproduce. For every case this writes its
//! packages side by side in a temp directory, builds the case's `build`
//! package as `comline build` resolves it (`compile_package`, dependencies
//! from the paths its config declares), and compares what the build gave with
//! the case's `expect`:
//!
//! - whether it built;
//! - for a build that did: every protocol of the package, the set it gives up
//!   (what is frozen with it), and where the case states it, who acknowledged
//!   a guarantee (the protocol's `@unverified` or the package's) and why; and
//!   the warnings the package's own schemas and config reported, by code and
//!   severity;
//! - for one that didn't: the errors that have a code, and text the others
//!   contain.
//!
//! A failed build reaches the caller as one rendered error, not a list of
//! diagnostics, so the codes of its errors are the `[code]` markers the
//! renderer writes (`Error[unverified::missing-marker]` with a span,
//! `[unverified::missing-marker]` without), found in that text, and each is
//! an error. Core has no coded error that isn't rendered that way. Warnings
//! are read structured, from the schemas' own.
//!
//! Every case is run, and every mismatch reported with its case's id, before
//! the test fails. A case core disagrees with, that core is wrong about, is in
//! `CORE_DISAGREES` with the rule it breaks: its mismatch is reported but
//! doesn't fail the test, and the case passing fails it, so the entry goes
//! when the bug does.
//!
//! The dependency cases need `deps`, which resolves a package's dependencies:
//! the module is built with it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use comline_core::package::build::compile_package;
use comline_core::schema::ir::frozen::unit::{ConfigValue, FrozenUnit, Guarantee};
use comline_core::schema::ir::unverified;
use serde_json::Value;

/// Cases where core's behaviour breaks the rule the case states, and the
/// case is right: id, and what is wrong. The runner holds each to still
/// disagreeing.
const CORE_DISAGREES: &[(&str, &str)] = &[];

fn cases_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/spec/cases")
}

/// A diagnostic as a case states it: its code and its severity, as written.
type Diag = (String, String);

/// What building the case's package gave, as far as a case speaks of it.
struct Outcome {
    builds: bool,
    /// The package's protocols: name, what it gives up, and for each guarantee
    /// the scope and reason it was acknowledged with.
    protocols: Vec<(String, Vec<Guarantee>, Vec<(Guarantee, &'static str, String)>)>,
    /// A build that passed: the package's own warnings. One that failed: the
    /// coded errors in what it said.
    diagnostics: BTreeSet<Diag>,
    /// A failed build's error, whole, for `error_contains`.
    error: String,
}

fn write_packages(root: &Path, case: &Value) {
    for package in case["packages"].as_array().unwrap() {
        let dir = root.join(package["name"].as_str().unwrap());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.idp"), package["config"].as_str().unwrap()).unwrap();
        for schema in package["schemas"].as_array().unwrap() {
            let path = dir.join(schema["path"].as_str().unwrap());
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, schema["text"].as_str().unwrap()).unwrap();
        }
    }
}

/// The namespace `src/<a>/<b>.ids` is compiled under: `a::b`.
fn namespace_of(path: &str) -> String {
    path.strip_prefix("src/").unwrap().strip_suffix(".ids").unwrap().replace('/', "::")
}

/// `text` without its ANSI colour sequences, which the renderer writes when it
/// believes it is on a terminal.
fn plain(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// The `[namespace::code]` markers in a rendered error: a lowercase
/// `namespace::kebab-name` in brackets.
fn coded(error: &str) -> BTreeSet<String> {
    let mut codes = BTreeSet::new();
    for (start, _) in error.match_indices('[') {
        let Some(end) = error[start..].find(']') else { continue };
        let inside = &error[start + 1..start + end];
        let Some((namespace, name)) = inside.split_once("::") else { continue };
        let word = |s: &str, extra: char| {
            s.starts_with(|c: char| c.is_ascii_lowercase())
                && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == extra)
        };
        if word(namespace, '_') && word(name, '-') {
            codes.insert(inside.to_string());
        }
    }
    codes
}

fn severity(severity: comline_core::diagnostics::Severity) -> &'static str {
    use comline_core::diagnostics::Severity;
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
        Severity::Hint => "hint",
    }
}

/// Who acknowledged `guarantee` for a protocol, and what they said: its own
/// `@unverified` (which wins where both say), else the package's.
fn acknowledgement(parameters: &[FrozenUnit], guarantee: Guarantee) -> Option<(&'static str, String)> {
    let own = match unverified::marker(parameters) {
        Some(ConfigValue::Str(reason)) => Some(reason),
        Some(ConfigValue::Dict(entries)) => entries.iter().find_map(|(key, value)| match value {
            ConfigValue::Str(reason) if key == guarantee.name() => Some(reason),
            _ => None,
        }),
        _ => None,
    };
    match own {
        Some(reason) => Some(("protocol", reason.clone())),
        None => unverified::package_reason_for(parameters, guarantee).map(|r| ("package", r.to_string())),
    }
}

fn build(case: &Value) -> Result<Outcome, String> {
    let tmp = tempfile::tempdir().unwrap();
    write_packages(tmp.path(), case);
    let name = case["build"].as_str().unwrap();
    let built = case["packages"].as_array().unwrap().iter().find(|p| p["name"] == name).unwrap();

    let context = match compile_package(&tmp.path().join(name)) {
        Ok(context) => context,
        Err(error) => {
            let error = plain(&format!("{error:?}"));
            let diagnostics = coded(&error).into_iter().map(|code| (code, "error".to_string())).collect();
            return Ok(Outcome { builds: false, protocols: vec![], diagnostics, error });
        }
    };

    // The package's own schemas come first, then its dependencies' and the
    // standard library's, which are the authors' of those to act on.
    let own: Vec<String> = built["schemas"].as_array().unwrap().iter().map(|s| namespace_of(s["path"].as_str().unwrap())).collect();
    let schemas: Vec<_> = context.schema_contexts.iter().take(own.len()).collect();
    let found: BTreeSet<String> = schemas.iter().map(|s| s.borrow().namespace_joined()).collect();
    if found != own.iter().cloned().collect() {
        return Err(format!("the first {} schemas of the build are {found:?}, not the package's own {own:?}", own.len()));
    }

    let mut protocols = vec![];
    let mut diagnostics = BTreeSet::new();
    for schema in schemas {
        let schema = schema.borrow();
        for warning in schema.warnings.borrow().iter() {
            diagnostics.insert((warning.code.id.to_string(), severity(warning.effective_severity()).to_string()));
        }
        for unit in schema.frozen_schema.borrow().as_ref().unwrap() {
            let FrozenUnit::Protocol { name, parameters, .. } = unit else { continue };
            let given_up = unverified::frozen_set(parameters).to_vec();
            let acknowledged = given_up
                .iter()
                .filter_map(|g| acknowledgement(parameters, *g).map(|(scope, reason)| (*g, scope, reason)))
                .collect();
            protocols.push((name.clone(), given_up, acknowledged));
        }
    }
    Ok(Outcome { builds: true, protocols, diagnostics, error: String::new() })
}

fn diagnostics_of(expect: &Value) -> BTreeSet<Diag> {
    expect["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| (d["code"].as_str().unwrap().to_string(), d["severity"].as_str().unwrap().to_string()))
        .collect()
}

fn names(set: impl IntoIterator<Item = Guarantee>) -> BTreeSet<&'static str> {
    set.into_iter().map(Guarantee::name).collect()
}

/// Every way `outcome` differs from what the case expects.
fn mismatches(case: &Value, outcome: &Outcome) -> Vec<String> {
    let expect = &case["expect"];
    let mut out = vec![];
    let builds = expect["builds"].as_bool().unwrap();
    if builds != outcome.builds {
        let error = if outcome.builds { String::new() } else { format!(": {}", outcome.error) };
        out.push(format!("builds: expected {builds}, got {}{error}", outcome.builds));
        return out;
    }

    let expected = diagnostics_of(expect);
    if expected != outcome.diagnostics {
        out.push(format!("diagnostics: expected {expected:?}, got {:?}", outcome.diagnostics));
    }
    if !builds {
        for text in expect["error_contains"].as_array().into_iter().flatten() {
            let text = text.as_str().unwrap();
            if !outcome.error.contains(text) {
                out.push(format!("error_contains: expected the error to contain {text:?}, got: {}", outcome.error));
            }
        }
        return out;
    }

    let expected: Vec<&Value> = expect["protocols"].as_array().unwrap().iter().collect();
    let expected_names: BTreeSet<&str> = expected.iter().map(|p| p["name"].as_str().unwrap()).collect();
    let got_names: BTreeSet<&str> = outcome.protocols.iter().map(|(name, ..)| name.as_str()).collect();
    if expected_names != got_names || expected.len() != outcome.protocols.len() {
        out.push(format!("protocols: expected {expected_names:?}, got {got_names:?}"));
    }
    for protocol in expected {
        let name = protocol["name"].as_str().unwrap();
        let Some((_, given_up, acknowledged)) = outcome.protocols.iter().find(|(n, ..)| n == name) else { continue };
        let want: BTreeSet<&str> = protocol["given_up"].as_array().unwrap().iter().map(|g| g.as_str().unwrap()).collect();
        let got = names(given_up.iter().copied());
        if want != got {
            out.push(format!("{name}: gives up: expected {want:?}, got {got:?}"));
        }
        for ack in protocol["acknowledged"].as_array().into_iter().flatten() {
            let guarantee = ack["guarantee"].as_str().unwrap();
            let Some((_, scope, reason)) = acknowledged.iter().find(|(g, ..)| g.name() == guarantee) else {
                out.push(format!("{name}: {guarantee}: expected an acknowledgement, got none"));
                continue;
            };
            let want_scope = ack["scope"].as_str().unwrap();
            if want_scope != *scope {
                out.push(format!("{name}: {guarantee}: scope: expected {want_scope}, got {scope}"));
            }
            if let Some(want_reason) = ack["reason"].as_str() {
                if want_reason != reason {
                    out.push(format!("{name}: {guarantee}: reason: expected {want_reason:?}, got {reason:?}"));
                }
            }
        }
    }
    out
}

#[test]
fn the_specifications_compiler_cases() {
    let mut files: Vec<PathBuf> = std::fs::read_dir(cases_dir())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "json"))
        .collect();
    files.sort();
    let mut ids = BTreeSet::new();
    let mut run = 0;
    let mut failures = vec![];
    for path in &files {
        let group = path.file_stem().unwrap().to_str().unwrap();
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let file: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(file["group"], group, "{}", path.display());
        for case in file["cases"].as_array().unwrap() {
            let id = case["id"].as_str().unwrap();
            assert!(id.starts_with(&format!("{group}/")), "{id} is in the wrong file: {group}.json");
            run += 1;
            ids.insert(id.to_string());
            let found = match build(case) {
                Ok(outcome) => mismatches(case, &outcome),
                Err(harness) => vec![format!("the runner: {harness}")],
            };
            match (CORE_DISAGREES.iter().find(|(known, _)| *known == id), found.is_empty()) {
                (None, false) => failures.push(format!("{id}\n    {}", found.join("\n    "))),
                (Some((_, rule)), false) => eprintln!("{id}: core is known to disagree ({rule}):\n    {}", found.join("\n    ")),
                (Some((_, rule)), true) => failures.push(format!("{id}\n    core agrees now: remove it from CORE_DISAGREES ({rule})")),
                (None, true) => {}
            }
        }
    }
    for (id, _) in CORE_DISAGREES {
        assert!(ids.contains(&id.to_string()), "CORE_DISAGREES has {id}, which is no case's");
    }
    assert!(run > 0, "no cases ran");
    assert!(failures.is_empty(), "{} of {run} cases differ:\n\n{}", failures.len(), failures.join("\n\n"));
}
