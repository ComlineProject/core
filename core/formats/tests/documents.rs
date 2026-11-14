//! A format's document and its ledger say the same things.

use std::fs;
use std::path::Path;

fn read(relative: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(relative))
        .unwrap_or_else(|e| panic!("{relative}: {e}"))
}

/// The ledger's rows: `(id, added, absent)`.
fn rows(format: u32) -> Vec<(String, String, String)> {
    let table: toml::Table = read(&format!("src/v{format}/ledger.toml"))
        .parse()
        .expect("ledger.toml is TOML");
    table["row"]
        .as_array()
        .expect("`row` is an array of tables")
        .iter()
        .map(|row| {
            let field = |name: &str| {
                row[name]
                    .as_str()
                    .unwrap_or_else(|| panic!("a row has no `{name}`: {row}"))
                    .to_string()
            };
            for name in ["commit", "date"] {
                field(name);
            }
            (field("id"), field("added"), field("absent"))
        })
        .collect()
}

#[test]
fn the_current_format_is_the_one_this_crate_ends_with() {
    assert_eq!(comline_ir_formats::CURRENT, 3);
    assert_eq!(comline_ir_formats::v3::FORMAT, 3);
}

#[test]
fn the_ledger_is_numbered_in_order_and_each_row_says_what_it_added_and_what_absence_reads_as() {
    let rows = rows(3);
    assert!(rows.len() >= 22, "the ledger is a history and only grows");
    for (n, (id, added, absent)) in rows.iter().enumerate() {
        assert_eq!(id, &format!("3.{:02}", n + 1));
        assert!(!added.is_empty() && !absent.is_empty(), "{id}");
    }
}

#[test]
fn the_document_has_each_ledger_row_and_only_those() {
    let schema = read("src/v3/SCHEMA.md");
    assert!(schema.starts_with("# IR format 3\n"));

    let table: Vec<Vec<&str>> = schema
        .lines()
        .filter(|l| l.starts_with("| 3."))
        .map(|l| l.trim_matches('|').split('|').map(str::trim).collect())
        .collect();
    let rows = rows(3);
    assert_eq!(
        table.len(),
        rows.len(),
        "SCHEMA.md's ledger table and ledger.toml differ in length"
    );
    for (cells, (id, added, absent)) in table.iter().zip(&rows) {
        assert_eq!(cells[0], id);
        assert_eq!(cells[3], added, "{id}: what was added");
        assert_eq!(cells[4], absent, "{id}: the absence reading");
    }
}

#[test]
fn every_row_a_document_cites_exists() {
    let schema = read("src/v3/SCHEMA.md");
    let ids: Vec<String> = rows(3).into_iter().map(|(id, _, _)| id).collect();
    for cited in schema.split("(3.").skip(1) {
        let id = format!("3.{}", &cited[..2]);
        assert!(
            ids.contains(&id),
            "SCHEMA.md cites {id}, which the ledger doesn't have"
        );
    }
}

#[test]
fn the_format_directory_has_its_four_files() {
    for file in ["SCHEMA.md", "types.rs", "codec.rs", "ledger.toml"] {
        assert!(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src/v3")
                .join(file)
                .is_file(),
            "{file}"
        );
    }
}

#[test]
fn nothing_is_sealed_while_format_3_is_current() {
    let manifest = read("MANIFEST");
    let sealed: Vec<&str> = manifest
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .collect();
    assert!(
        sealed.is_empty(),
        "format 3 is current and is sealed when format 4 is introduced: {sealed:?}"
    );
}
