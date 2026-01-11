use comline_core::package::config::idl::grammar;
use comline_core::package::config::dependency::DependencyConfig;

#[test]
fn test_parse_dependencies_from_config() {
    let config_source = r#"
congregation myapi
specification_version = 1

dependencies = {
    stdlib = {
        version = "1.0.0"
        uri = "comline://registry.comline.io/stdlib"
        hash = "blake3:abc123"
    }
    
    uuid = {
        version = "4.1.0"
        uri = "comline://registry.comline.io/uuid"
        hash = "blake3:def456"
    }
    
    local_lib = {
        path = "../mylib"
    }
}
"#;

    // Parse the config
    let parsed = grammar::parse(config_source).expect("Failed to parse config");
    
    // Extract dependencies
    let deps = DependencyConfig::parse_dependencies(&parsed.assignments);
    
    // Verify we got the dependencies
    assert_eq!(deps.len(), 3, "Should have 3 dependencies");
    
    assert!(deps.contains_key("stdlib"));
    assert!(deps.contains_key("uuid"));
    assert!(deps.contains_key("local_lib"));
    
    // Check stdlib details
    let stdlib = deps.get("stdlib").unwrap();
    match &stdlib.source {
        comline_core::package::config::dependency::DependencySource::Registry { version, uri, hash, .. } => {
            assert_eq!(version, "1.0.0");
            assert_eq!(uri, "comline://registry.comline.io/stdlib");
            assert_eq!(hash.as_ref().unwrap(), "blake3:abc123");
        }
        _ => panic!("Expected Registry source"),
    }
    
    // Check local_lib is recognized as Path
    let local_lib = deps.get("local_lib").unwrap();
    match &local_lib.source {
        comline_core::package::config::dependency::DependencySource::Path { path } => {
            assert_eq!(path.to_str().unwrap(), "../mylib");
        }
        _ => panic!("Expected Path source"),
    }
    
    println!("✅ Dependency parsing test passed!");
}
