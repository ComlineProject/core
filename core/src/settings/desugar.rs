use super::value::{SettingsDict, SettingsValue};

/// Two dotted settings keys in the same block that can't coexist: either
/// one path is used as both a leaf and a nested group (`a.b = true` and
/// `a.b.c = 1`), or the exact same leaf path is written twice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesugarConflict {
    /// The path already committed to one shape, as written.
    pub existing_path: String,
    /// The path that conflicts with it, as written.
    pub conflicting_path: String,
}

/// Turn a flat list of `(dotted path, leaf value)` pairs - one per
/// `Setting` entry, in source order - into the nested tree they sugar for.
/// Collects every conflict found rather than stopping at the first, so a
/// caller can report them all at once.
pub fn desugar(
    entries: &[(Vec<String>, SettingsValue)],
) -> Result<SettingsDict, Vec<DesugarConflict>> {
    let mut root = SettingsDict::new();
    let mut conflicts = Vec::new();
    for (path, value) in entries {
        if let Err(c) = insert_path(&mut root, path, value.clone()) {
            conflicts.push(c);
        }
    }
    if conflicts.is_empty() {
        Ok(root)
    } else {
        Err(conflicts)
    }
}

/// Same algorithm, but a conflict silently keeps whichever value got there
/// first and drops the later entry, rather than erroring. For callers that
/// run without the dedicated conflict-checking pre-pass
/// (`schema::ir::compiler::settings_resolution::check_settings_conflicts`)
/// and so can't surface a real diagnostic - e.g. a one-off in-memory
/// interpretation that skips that pre-pass entirely.
pub fn desugar_lenient(entries: &[(Vec<String>, SettingsValue)]) -> SettingsDict {
    let mut root = SettingsDict::new();
    for (path, value) in entries {
        let _ = insert_path(&mut root, path, value.clone());
    }
    root
}

fn insert_path(
    dict: &mut SettingsDict,
    path: &[String],
    value: SettingsValue,
) -> Result<(), DesugarConflict> {
    match path {
        [] => unreachable!("a Setting's key always has at least one segment"),
        [last] => match dict.0.get(last) {
            Some(SettingsValue::Dict(_)) => Err(DesugarConflict {
                existing_path: format!("{last}.*"),
                conflicting_path: last.clone(),
            }),
            Some(_) => Err(DesugarConflict {
                existing_path: last.clone(),
                conflicting_path: last.clone(),
            }),
            None => {
                dict.0.insert(last.clone(), value);
                Ok(())
            }
        },
        [first, rest @ ..] => {
            let full_path = || path.join(".");
            let entry = dict
                .0
                .entry(first.clone())
                .or_insert_with(|| SettingsValue::Dict(SettingsDict::new()));
            match entry {
                SettingsValue::Dict(sub) => insert_path(sub, rest, value).map_err(|c| {
                    DesugarConflict {
                        existing_path: format!("{first}.{}", c.existing_path),
                        conflicting_path: format!("{first}.{}", c.conflicting_path),
                    }
                }),
                _ => Err(DesugarConflict {
                    existing_path: first.clone(),
                    conflicting_path: full_path(),
                }),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(v: bool) -> SettingsValue {
        SettingsValue::Bool(v)
    }

    fn path(s: &str) -> Vec<String> {
        s.split('.').map(str::to_string).collect()
    }

    #[test]
    fn flat_keys_produce_a_flat_dict() {
        let dict = desugar(&[(path("a"), b(true)), (path("b"), b(false))]).unwrap();
        assert_eq!(dict.get("a"), Some(&b(true)));
        assert_eq!(dict.get("b"), Some(&b(false)));
    }

    #[test]
    fn a_dotted_key_produces_nesting() {
        let dict = desugar(&[(path("a.b.c"), b(true))]).unwrap();
        let SettingsValue::Dict(a) = dict.get("a").unwrap() else {
            panic!("expected a dict at 'a'");
        };
        let SettingsValue::Dict(b_dict) = a.get("b").unwrap() else {
            panic!("expected a dict at 'a.b'");
        };
        assert_eq!(b_dict.get("c"), Some(&b(true)));
    }

    #[test]
    fn leaf_then_group_conflicts() {
        let err = desugar(&[(path("a.b"), b(true)), (path("a.b.c"), b(true))]).unwrap_err();
        assert_eq!(err.len(), 1);
        assert_eq!(err[0].existing_path, "a.b");
        assert_eq!(err[0].conflicting_path, "a.b.c");
    }

    #[test]
    fn group_then_leaf_conflicts_too() {
        let err = desugar(&[(path("a.b.c"), b(true)), (path("a.b"), b(true))]).unwrap_err();
        assert_eq!(err.len(), 1);
    }

    #[test]
    fn sibling_paths_sharing_a_prefix_do_not_conflict() {
        let dict = desugar(&[(path("a.b"), b(true)), (path("a.c"), b(false))]).unwrap();
        let SettingsValue::Dict(a) = dict.get("a").unwrap() else {
            panic!("expected a dict at 'a'");
        };
        assert_eq!(a.get("b"), Some(&b(true)));
        assert_eq!(a.get("c"), Some(&b(false)));
    }

    #[test]
    fn exact_duplicate_leaf_conflicts() {
        let err = desugar(&[(path("a"), b(true)), (path("a"), b(false))]).unwrap_err();
        assert_eq!(err.len(), 1);
        assert_eq!(err[0].existing_path, "a");
        assert_eq!(err[0].conflicting_path, "a");
    }

    #[test]
    fn lenient_never_errors_on_conflicting_input() {
        let dict = desugar_lenient(&[(path("a.b"), b(true)), (path("a.b.c"), b(false))]);
        // Deterministic: the first entry (`a.b = true`) won; the
        // conflicting second one was dropped, not panicked on.
        let SettingsValue::Dict(a) = dict.get("a").unwrap() else {
            panic!("expected a dict at 'a'");
        };
        assert_eq!(a.get("b"), Some(&b(true)));
    }
}
