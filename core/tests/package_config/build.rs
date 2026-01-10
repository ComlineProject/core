// Standard Uses

// Crate Uses
use crate::package_config::TEST_PACKAGE_DIR;

// External Uses
use comline_core::package;


#[test]
#[ignore] // TODO: Update for rust-sitter parser - uses from_origin
fn build_package() {
    package::build::build(&TEST_PACKAGE_DIR).unwrap();
}
