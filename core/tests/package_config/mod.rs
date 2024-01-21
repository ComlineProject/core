// Relative Modules
mod parse;
mod compile;
mod build;

// Standard Uses
use std::path::{Path, PathBuf};

// Crate Uses

// External Uses
use comline_core::package::config::idl::constants::CONGREGATION_EXTENSION;

use once_cell::sync::Lazy;


static TEST_PACKAGE_DIR: Lazy<&Path> = Lazy::new(|| Path::new("../__TEST_DATA__/test/"));
static TEST_PACKAGE_CONFIG_PATH: Lazy<PathBuf> = Lazy::new(||
    TEST_PACKAGE_DIR.join(format!("config.{}", CONGREGATION_EXTENSION))
);


/*
#[test]
fn parse_package_project() {
    let config = config::idl::parser::from_path(&TEST_PACKAGE_CFG_PATH).unwrap();

    pretty_assertions::assert_eq!(
        config,
        (
            VIndex {
                meta: UnitIndex::Index { path: "".to_string(), source: "".to_string() },
                nodes: vec![]
            },
            vec![
                ASTUnit::Congregation("".to_owned()),
                ASTUnit::Dependencies(vec![

                ])
            ]
        )
    )
}
*/
