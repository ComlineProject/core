// Standard Uses

// Crate Uses
use crate::package_config::TEST_PACKAGE_DIR;

// External Uses
use comline_core::package;


#[test]
fn build_package() {
    package::build::build(&TEST_PACKAGE_DIR).unwrap();
}
