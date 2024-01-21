// Standard Uses

// Crate Uses
use crate::package_config::TEST_PACKAGE_CONFIG_PATH;

// External Uses
use comline_core::package::config::ir::compiler::Compile;
use comline_core::package::config::ir::interpreter::ProjectInterpreter;


#[test]
fn compile_test_package_package_from_config() {
    let compiled = ProjectInterpreter::from_origin(&TEST_PACKAGE_CONFIG_PATH)
        .unwrap();

    // pretty_assertions::assert_eq!(compiled, ());
    
    todo!()
}
