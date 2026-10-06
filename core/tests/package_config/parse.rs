use comline_core::package::config::idl::grammar;

#[test]
fn test_parse_simple_congregation() {
    let code = r#"
congregation MyProject

version = "1.0.0"
active = true
count = 42
"#;
    let result = grammar::parse(code);
    assert!(result.is_ok(), "Failed to parse simple congregation: {:?}", result.err());
    
    let congregation = result.unwrap();
    assert_eq!(congregation.name.value, "MyProject");
    assert_eq!(congregation.assignments.len(), 3);
}

#[test]
fn test_parse_list() {
    let code = r#"
congregation Lists

items = ["a", "b", "c"]
numbers = [1, 2, 3]
mixed = ["a", 1, true]
"#;
    let result = grammar::parse(code);
    assert!(result.is_ok());
    let congregation = result.unwrap();
    assert_eq!(congregation.assignments.len(), 3);
}

#[test]
fn test_parse_dictionary() {
    let code = r#"
congregation Config

database = {
    host = "localhost"
    port = 5432
    enabled = true
}
"#;
    let result = grammar::parse(code);
    assert!(result.is_ok());
    
    let congregation = result.unwrap();
    // Verify structure deep access later with helpers
    assert_eq!(congregation.assignments.len(), 1);
}

#[test]
fn test_parse_comments() {
    let code = r#"
// This is a comment
congregation WithComments

/* Block comment */
key = "value" // Inline comment
"#;
    let result = grammar::parse(code);
    assert!(result.is_ok());
}

#[test]
fn test_nested_complex() {
    let code = r#"
congregation Complex

servers = [
    {
        name = "primary"
        ip = "10.0.0.1"
    },
    {
        name = "backup"
        ip = "10.0.0.2"
    }
]
"#;
    let result = grammar::parse(code);
    assert!(result.is_ok());
}

#[test]
fn test_complex_keys() {
    let code = r#"
congregation KeysTest

// Namespaced
std::publish::REGISTRY = "main"

// Item Version Meta
python#3.12.0 = { path = "/usr/bin/python" }
lib::foo#1.0 = true

// Dependency Address
std@latest = "std"
foo::bar@1.0.0 = "bar"
"#;
    let result = grammar::parse(code);
    assert!(result.is_ok(), "Failed to parse complex keys: {:?}", result.err());
    let config = result.unwrap();
    assert_eq!(config.assignments.len(), 5);
}

#[test]
fn test_unquoted_identifiers() {
    let code = r#"
congregation Identifiers

mode = all
status = active
list = [ one, two, three ]
"#;
    let result = grammar::parse(code);
    assert!(result.is_ok(), "Failed to parse unquoted identifiers: {:?}", result.err());
    let config = result.unwrap();
    
    // Check values
    // mode = all -> Identifier(all)
    // list -> List([Identifier(one), ...])
}

#[test]
fn test_real_world_structure() {
    let code = r#"
congregation TestProject
specification_version = 1

code_generation = {
    languages = {
        python#3.11 = {}
        rust#1.70 = {}
    }
}

publish_registries = {
    mainstream = std::publish::MAINSTREAM_REGISTRY
    local = { uri = "local://tmp" }
}
"#;
    let result = grammar::parse(code);
    assert!(result.is_ok(), "Failed to parse real world structure: {:?}", result.err());
}

#[test]
fn test_invalid_syntax() {
    let code = r#"
congregation Invalid
key without equals
"#;
    let result = grammar::parse(code);
    assert!(result.is_err());
}

#[test]
fn assignments_carry_real_spans_at_every_nesting_depth() {
    // Needed for position-based hover: every `Assignment`, top-level and
    // nested inside a `Dictionary`, must carry a byte span that actually
    // covers its own `key = value` text and nests correctly inside its
    // parent's.
    let code = "congregation Spans\nsettings = {\n    validators = {\n        allowed = false\n    }\n}\n";
    let congregation = grammar::parse(code).expect("should parse");

    let settings = &congregation.assignments[0];
    assert_eq!(&code[settings.span.0..settings.span.1], "settings = {\n    validators = {\n        allowed = false\n    }\n}");

    let grammar::Value::Dictionary(settings_dict) = &settings.value.value else {
        panic!("settings should be a dictionary");
    };
    let validators = &settings_dict.assignments[0];
    assert_eq!(&code[validators.span.0..validators.span.1], "validators = {\n        allowed = false\n    }");
    assert!(validators.span.0 > settings.span.0, "nested span should start after its parent's");
    assert!(validators.span.1 < settings.span.1, "nested span should end before its parent's");

    let grammar::Value::Dictionary(validators_dict) = &validators.value.value else {
        panic!("validators should be a dictionary");
    };
    let allowed = &validators_dict.assignments[0];
    assert_eq!(&code[allowed.span.0..allowed.span.1], "allowed = false");
    assert!(allowed.span.0 > validators.span.0 && allowed.span.1 < validators.span.1);
}
