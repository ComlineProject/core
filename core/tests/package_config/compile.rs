// Standard Uses

// Crate Uses
use crate::package_config::TEST_PACKAGE_CONFIG_PATH;

// External Uses
use std::path::Path;
use comline_core::package::config::ir::compiler::Compile;
use comline_core::package::config::ir::interpreter::ProjectInterpreter;


#[test]
// #[ignore]
fn compile_test_package_package_from_config() {
    let result = ProjectInterpreter::from_origin(Path::new(&*TEST_PACKAGE_CONFIG_PATH));
    
    assert!(result.is_ok(), "Failed to compile package config: {:?}", result.err());
    let compiled = result.unwrap();
    
    assert_eq!(compiled.config.name.value, "test"); // config.idp has "congregation test"
    // Verify frozen config if possible, or just compilation success
}
