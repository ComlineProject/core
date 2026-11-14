//! What a format change has to come with.
//!
//! - Always: the current format has its directory with the four files, and
//!   the seals hold.
//! - A bump of `CURRENT_IR_FORMAT_VERSION` (against the merge base) is one
//!   format at a time, and comes with the new format's directory including
//!   its step (`step.rs`, with `UNKNOWN`, `up` and `down`), the previous
//!   format's seal, and a fixture for every ledger row of the previous
//!   format.
//! - Without a bump, a move of the golden fresh-build hashes
//!   (`core/tests/migrations/expected/`) needs a new ledger row, and a ledger
//!   row needs `SCHEMA.md` and the frozen copy changed in the same change.

use std::path::Path;

use crate::repo::{current_format_in, Repo, EXPECTED, FORMAT_FILES, STAMP_FILE};
use crate::seal;

/// What the step of a bump has to define, as it appears in `step.rs`.
const STEP_PARTS: [(&str, &str); 3] = [
    ("UNKNOWN", "the list of paths it writes as unknown"),
    ("fn up", "`up`"),
    ("fn down", "`down`, the witness"),
];

/// The fixtures of format `format`'s stores.
fn stores_dir(repo: &Repo, format: u32) -> std::path::PathBuf {
    repo.path(&format!("core/tests/migrations/stores/format-{format}"))
}

/// One row of a format's ledger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    /// The fixture that has the row, a directory under
    /// `tests/migrations/stores/format-<N>/`.
    pub fixture: Option<String>,
}

/// The rows of a `ledger.toml`.
pub fn ledger(text: &str) -> Result<Vec<Row>, String> {
    let table: toml::Table = text.parse().map_err(|e| format!("ledger.toml: {e}"))?;
    let rows = match table.get("row") {
        None => return Ok(vec![]),
        Some(toml::Value::Array(rows)) => rows,
        Some(_) => return Err("ledger.toml: `row` is not an array of tables".into()),
    };
    rows.iter()
        .map(|row| {
            let field = |name: &str| row.get(name).and_then(|v| v.as_str()).map(str::to_string);
            Ok(Row {
                id: field("id").ok_or("ledger.toml: a row has no `id`")?,
                fixture: field("fixture"),
            })
        })
        .collect()
}

fn ledger_of(repo: &Repo, format: u32) -> Result<Vec<Row>, String> {
    ledger(&repo.read(&format!("core/formats/src/v{format}/ledger.toml"))?)
}

fn missing_files(repo: &Repo, format: u32, files: &[&str]) -> Vec<String> {
    let dir = repo.format_dir(format);
    files
        .iter()
        .filter(|f| !dir.join(f).is_file())
        .map(|f| format!("format {format} has no `{f}` in core/formats/src/v{format}/"))
        .collect()
}

/// Everything the change from `base` (a revision) to `HEAD` is missing; with
/// no `base`, only what is true of the tree itself.
pub fn check(repo: &Repo, base: Option<&str>) -> Result<Vec<String>, String> {
    let current = repo.current_format()?;
    let mut problems = missing_files(repo, current, &FORMAT_FILES);
    problems.extend(seal::verify(repo)?);

    let Some(base) = base else {
        return Ok(problems);
    };
    let merge_base = repo.git(&["merge-base", base, "HEAD"])?.trim().to_string();
    let before = current_format_in(
        &repo
            .file_at(&merge_base, STAMP_FILE)
            .ok_or_else(|| format!("{STAMP_FILE} doesn't exist at {base}"))?,
    )?;
    if current < before {
        problems.push(format!(
            "CURRENT_IR_FORMAT_VERSION went down, from {before} to {current}"
        ));
    } else if current > before {
        problems.extend(bump(repo, before, current)?);
    } else {
        problems.extend(no_bump(repo, &merge_base, current)?);
    }
    Ok(problems)
}

/// A bump from `before` to `current`.
fn bump(repo: &Repo, before: u32, current: u32) -> Result<Vec<String>, String> {
    let mut problems = vec![];
    if current != before + 1 {
        problems.push(format!(
            "CURRENT_IR_FORMAT_VERSION went from {before} to {current}: a change moves one format, with a step each"
        ));
        return Ok(problems);
    }

    problems.extend(missing_files(repo, current, &["step.rs"]));
    if let Ok(step) = std::fs::read_to_string(repo.format_dir(current).join("step.rs")) {
        for (part, what) in STEP_PARTS {
            if !step.contains(part) {
                problems.push(format!(
                    "the step {before} -> {current} (v{current}/step.rs) doesn't define {what}"
                ));
            }
        }
    }
    if let Ok(schema) = std::fs::read_to_string(repo.format_dir(current).join("SCHEMA.md")) {
        if !schema
            .lines()
            .any(|l| l.starts_with(&format!("# IR format {current}")))
        {
            problems.push(format!(
                "v{current}/SCHEMA.md doesn't start a `# IR format {current}` document"
            ));
        }
    }

    // The seal of `before` is checked with every seal, by `seal::verify`.
    for row in ledger_of(repo, before)? {
        match &row.fixture {
            None => problems.push(format!(
                "ledger row {} of format {before} has no fixture: the step {before} -> {current} is proved on a store of each level",
                row.id
            )),
            Some(fixture) if !Path::new(&stores_dir(repo, before)).join(fixture).is_dir() => {
                problems.push(format!(
                    "ledger row {} of format {before} names the fixture `{fixture}`, which isn't in core/tests/migrations/stores/format-{before}/",
                    row.id
                ))
            }
            Some(_) => {}
        }
    }
    Ok(problems)
}

/// A change that leaves the format number alone.
fn no_bump(repo: &Repo, merge_base: &str, current: u32) -> Result<Vec<String>, String> {
    let mut problems = vec![];
    let changed: Vec<String> = repo
        .git(&["diff", "--name-only", merge_base, "HEAD"])?
        .lines()
        .map(str::to_string)
        .collect();
    let touched = |path: &str| changed.iter().any(|c| c == path);

    let ledger_path = format!("core/formats/src/v{current}/ledger.toml");
    let rows_before = match repo.file_at(merge_base, &ledger_path) {
        Some(text) => ledger(&text)?.len(),
        None => 0,
    };
    let rows_now = ledger_of(repo, current)?.len();

    if rows_now < rows_before {
        problems.push(format!(
            "a row was removed from format {current}'s ledger: it is a history"
        ));
    }
    if rows_now > rows_before {
        let dir = format!("core/formats/src/v{current}");
        if !touched(&format!("{dir}/SCHEMA.md")) {
            problems.push(format!("format {current}'s ledger has a new row, and SCHEMA.md of the format didn't change with it"));
        }
        if !touched(&format!("{dir}/types.rs")) && !touched(&format!("{dir}/codec.rs")) {
            problems.push(format!("format {current}'s ledger has a new row, and the frozen copy (types.rs, codec.rs) didn't change with it"));
        }
    }
    if changed
        .iter()
        .any(|c| c.starts_with(&format!("{EXPECTED}/")))
        && rows_now <= rows_before
    {
        problems.push(format!(
            "the golden fresh-build hashes in {EXPECTED}/ moved, and format {current}'s ledger has no new row: \
             a change to what a build writes is a ledger row (with the same lines in the frozen copy) or a bump"
        ));
    }
    Ok(problems)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::fs;
    use std::ops::Deref;

    /// A repository on disk, removed when dropped.
    pub struct Scratch {
        _dir: tempfile::TempDir,
        repo: Repo,
    }

    impl Deref for Scratch {
        type Target = Repo;
        fn deref(&self) -> &Repo {
            &self.repo
        }
    }

    fn write(repo: &Repo, relative: &str, text: &str) {
        let path = repo.path(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn format_files(repo: &Repo, format: u32) {
        let dir = format!("core/formats/src/v{format}");
        write(
            repo,
            &format!("{dir}/SCHEMA.md"),
            &format!("# IR format {format}\n"),
        );
        write(repo, &format!("{dir}/types.rs"), "// types\n");
        write(repo, &format!("{dir}/codec.rs"), "// codec\n");
        write(repo, &format!("{dir}/ledger.toml"), "[[row]]\nid = \"1\"\n");
    }

    /// A tree whose current format is `current`, with a directory for each
    /// format up to it, no seals and no git.
    pub fn scratch(current: u32) -> Scratch {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repo::new(dir.path());
        write(
            &repo,
            STAMP_FILE,
            &format!("pub const CURRENT_IR_FORMAT_VERSION: u32 = {current};\n"),
        );
        for format in 3..=current {
            format_files(&repo, format);
        }
        write(&repo, crate::repo::MANIFEST, "# seals\n");
        write(&repo, &format!("{EXPECTED}/tour.txt"), "tree 00\n");
        Scratch { _dir: dir, repo }
    }

    fn commit(repo: &Repo, message: &str) {
        repo.git(&["add", "-A"]).unwrap();
        repo.git(&[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-q",
            "-m",
            message,
        ])
        .unwrap();
    }

    /// A committed tree at format 3, tagged `base`.
    fn based() -> Scratch {
        let repo = scratch(3);
        repo.git(&["init", "-q", "-b", "main"]).unwrap();
        commit(&repo, "base");
        repo.git(&["tag", "base"]).unwrap();
        repo
    }

    fn contains(problems: &[String], text: &str) -> bool {
        problems.iter().any(|p| p.contains(text))
    }

    #[test]
    fn a_change_that_leaves_the_formats_alone_passes() {
        let repo = based();
        write(&repo, "README.md", "unrelated\n");
        commit(&repo, "unrelated");
        assert_eq!(check(&repo, Some("base")).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn the_current_format_has_its_four_files() {
        let repo = based();
        fs::remove_file(repo.format_dir(3).join("codec.rs")).unwrap();
        let problems = check(&repo, None).unwrap();
        assert!(
            contains(&problems, "format 3 has no `codec.rs`"),
            "{problems:?}"
        );
    }

    #[test]
    fn a_bump_without_its_step_its_seal_or_its_fixtures_fails_and_with_them_passes() {
        let repo = based();
        write(
            &repo,
            STAMP_FILE,
            "pub const CURRENT_IR_FORMAT_VERSION: u32 = 4;\n",
        );
        format_files(&repo, 4);
        commit(&repo, "bump");

        let problems = check(&repo, Some("base")).unwrap();
        assert!(
            contains(&problems, "format 4 has no `step.rs`"),
            "{problems:?}"
        );
        assert!(
            contains(&problems, "format 3 is superseded by 4 and not sealed"),
            "{problems:?}"
        );
        assert!(
            contains(&problems, "ledger row 1 of format 3 has no fixture"),
            "{problems:?}"
        );

        // A step without the witness is not a step.
        write(
            &repo,
            "core/formats/src/v4/step.rs",
            "pub const UNKNOWN: &[&str] = &[];\npub fn up() {}\n",
        );
        let problems = check(&repo, Some("base")).unwrap();
        assert!(
            contains(&problems, "doesn't define `down`, the witness"),
            "{problems:?}"
        );

        write(
            &repo,
            "core/formats/src/v4/step.rs",
            "pub const UNKNOWN: &[&str] = &[];\npub fn up() {}\npub fn down() {}\n",
        );
        seal::seal(&repo, 3, "format 4 is introduced").unwrap();
        write(
            &repo,
            "core/formats/src/v3/ledger.toml",
            "[[row]]\nid = \"1\"\nfixture = \"tour\"\n",
        );
        // Editing the ledger changed format 3 after it was sealed: seal last.
        seal::seal(&repo, 3, "format 4 is introduced").unwrap();
        let problems = check(&repo, Some("base")).unwrap();
        assert!(
            contains(&problems, "names the fixture `tour`, which isn't in"),
            "{problems:?}"
        );

        write(
            &repo,
            "core/tests/migrations/stores/format-3/tour/PRODUCED_BY",
            "rev\n",
        );
        commit(&repo, "bump, complete");
        assert_eq!(check(&repo, Some("base")).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn a_bump_skipping_a_format_fails() {
        let repo = based();
        write(
            &repo,
            STAMP_FILE,
            "pub const CURRENT_IR_FORMAT_VERSION: u32 = 5;\n",
        );
        format_files(&repo, 5);
        commit(&repo, "bump");
        let problems = check(&repo, Some("base")).unwrap();
        assert!(contains(&problems, "went from 3 to 5"), "{problems:?}");
    }

    #[test]
    fn golden_hashes_that_moved_need_a_ledger_row_with_the_frozen_copy() {
        let repo = based();
        write(&repo, &format!("{EXPECTED}/tour.txt"), "tree 01\n");
        commit(&repo, "moved");
        let problems = check(&repo, Some("base")).unwrap();
        assert!(contains(&problems, "ledger has no new row"), "{problems:?}");

        // A row alone is not enough: the document and the frozen copy change with it.
        write(
            &repo,
            "core/formats/src/v3/ledger.toml",
            "[[row]]\nid = \"1\"\n[[row]]\nid = \"2\"\n",
        );
        commit(&repo, "row");
        let problems = check(&repo, Some("base")).unwrap();
        assert!(
            contains(&problems, "SCHEMA.md of the format didn't change"),
            "{problems:?}"
        );
        assert!(
            contains(
                &problems,
                "the frozen copy (types.rs, codec.rs) didn't change"
            ),
            "{problems:?}"
        );

        write(
            &repo,
            "core/formats/src/v3/SCHEMA.md",
            "# IR format 3\nrow 2\n",
        );
        write(
            &repo,
            "core/formats/src/v3/types.rs",
            "// types\n// row 2\n",
        );
        commit(&repo, "row, document and copy");
        assert_eq!(check(&repo, Some("base")).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn a_ledger_row_cannot_be_removed() {
        let repo = based();
        write(&repo, "core/formats/src/v3/ledger.toml", "");
        commit(&repo, "removed");
        let problems = check(&repo, Some("base")).unwrap();
        assert!(contains(&problems, "a row was removed"), "{problems:?}");
    }

    #[test]
    fn a_ledger_is_read() {
        let rows =
            ledger("[[row]]\nid = \"3.01\"\nfixture = \"tour\"\n[[row]]\nid = \"3.02\"\n").unwrap();
        assert_eq!(
            rows[0],
            Row {
                id: "3.01".into(),
                fixture: Some("tour".into())
            }
        );
        assert_eq!(
            rows[1],
            Row {
                id: "3.02".into(),
                fixture: None
            }
        );
        assert_eq!(ledger("# none\n").unwrap(), vec![]);
        assert!(ledger("[[row]]\nfixture = \"x\"\n").is_err());
    }
}
