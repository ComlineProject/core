use std::path::{Path, PathBuf};

/// `SCHEMAS` lists every schema of `packages/std/src` — a new file can't be
/// left out of the embedded std by forgetting to list it.
#[test]
fn every_std_schema_is_embedded() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("packages/std/src");
    let mut on_disk = Vec::new();
    collect(&src, &src, &mut on_disk);
    on_disk.sort();

    let mut embedded: Vec<String> = comline_core_stdlib::SCHEMAS.iter().map(|(path, _)| path.to_string()).collect();
    embedded.sort();
    assert_eq!(embedded, on_disk);
}

fn collect(root: &Path, dir: &Path, found: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path: PathBuf = entry.unwrap().path();
        if path.is_dir() {
            collect(root, &path, found);
        } else if path.extension().is_some_and(|e| e == "ids") {
            let relative = path.strip_prefix(root).unwrap().with_extension("");
            found.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
}
