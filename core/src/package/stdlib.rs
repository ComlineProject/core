// The standard library: `use std::…` in any package.
//
// Its schemas are the `std` package embedded by `comline-core-stdlib`, versioned
// with the toolchain. A build merges in the std schemas its imports reach,
// namespaced under `std` the way a dependency is under its declared name - and
// only those, so a package that doesn't use std compiles, freezes and generates
// exactly as it would without it. No filesystem and no network: the playground
// and the language server get the same std as the CLI.

// Standard Uses
use std::collections::HashMap;

// Crate Uses
use crate::schema::idl::grammar::{self, Declaration};
use crate::schema::ir::compiler::import_resolver::ImportResolver;

/// The namespace every std schema lives under.
pub const NAMESPACE: &str = "std";

/// Every std schema: its namespace (`std` first) and its source.
pub fn schemas() -> impl Iterator<Item = (Vec<String>, &'static str)> {
    comline_core_stdlib::SCHEMAS.iter().map(|(path, source)| {
        let mut namespace = vec![NAMESPACE.to_string()];
        namespace.extend(path.split('/').map(String::from));
        (namespace, *source)
    })
}

/// The std schemas `sources` import, directly or through each other, as
/// `(namespace, source)` pairs ready to merge in after them.
pub(crate) fn used_by(sources: &[(Vec<String>, String)]) -> Vec<(Vec<String>, String)> {
    let catalog: Vec<(Vec<String>, &str)> = schemas().collect();
    let mut wanted = vec![false; catalog.len()];
    let mut pending: Vec<Vec<String>> = sources
        .iter()
        .flat_map(|(namespace, source)| std_imports(namespace, source))
        .collect();

    while let Some(target) = pending.pop() {
        if let Some(index) = schema_for(&catalog, &target) {
            if !wanted[index] {
                wanted[index] = true;
                pending.extend(std_imports(&catalog[index].0, catalog[index].1));
            }
        }
    }

    catalog
        .into_iter()
        .zip(wanted)
        .filter(|(_, wanted)| *wanted)
        .map(|((namespace, source), _)| (namespace, source.to_string()))
        .collect()
}

/// The std namespaces the `use`s in `source` (a schema at `namespace`) point
/// at. A schema that doesn't parse has none; it reports its own parse error.
fn std_imports(namespace: &[String], source: &str) -> Vec<Vec<String>> {
    let Ok(document) = grammar::parse(source) else {
        return vec![];
    };
    let resolver = ImportResolver::new(vec![], HashMap::new(), None);
    document
        .0
        .iter()
        .filter_map(|decl| match &decl.value {
            Declaration::Use(use_stmt) => resolver.resolve_namespace(&use_stmt.path, namespace).ok(),
            _ => None,
        })
        .map(|resolved| resolved.absolute_namespace)
        .filter(|target| target.first().map(String::as_str) == Some(NAMESPACE))
        .collect()
}

/// The schema a `use` of `target` resolves to, the way the build matches it:
/// the longest namespace that prefixes it (`std::http::Request` → `std::http`).
fn schema_for(catalog: &[(Vec<String>, &str)], target: &[String]) -> Option<usize> {
    (0..catalog.len())
        .filter(|&i| target.starts_with(&catalog[i].0))
        .max_by_key(|&i| catalog[i].0.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn namespaces(sources: &[(Vec<String>, String)]) -> Vec<String> {
        let mut names: Vec<String> = used_by(sources).into_iter().map(|(ns, _)| ns.join("::")).collect();
        names.sort();
        names
    }

    fn schema(namespace: &[&str], source: &str) -> (Vec<String>, String) {
        (namespace.iter().map(|s| s.to_string()).collect(), source.to_string())
    }

    #[test]
    fn only_what_is_imported_is_merged() {
        assert!(namespaces(&[schema(&["api"], "struct A {\n    id: u64\n}\n")]).is_empty());
        assert_eq!(namespaces(&[schema(&["api"], "use std::http::Request\n")]), ["std::http"]);
        assert_eq!(
            namespaces(&[
                schema(&["api"], "use std::http\n"),
                schema(&["auth"], "use std::validators::{StringBounds}\n"),
            ]),
            ["std::http", "std::validators"]
        );
    }

    #[test]
    fn an_unknown_std_path_merges_nothing() {
        // `check_imports` then reports it, with a suggestion from std.
        assert!(namespaces(&[schema(&["api"], "use std::htp::Request\n")]).is_empty());
    }

    #[test]
    fn every_std_schema_parses() {
        for (namespace, source) in schemas() {
            assert!(grammar::parse(source).is_ok(), "std schema `{}` doesn't parse", namespace.join("::"));
        }
    }
}
