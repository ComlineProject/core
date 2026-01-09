// Quick test to verify the parser works

use comline_core::schema::idl::grammar;

fn main() {
    let source = r#"
struct Message {
    sender: str
    content: str
}

enum Status {
    Ok
    Error
}

protocol Chat {
    function send(Message) returns Status
}
"#;

    match grammar::parse(source) {
        Ok(ast) => {
            println!("✅ Parsing succeeded!");
            println!("AST: {:#?}", ast);
        }
        Err(e) => {
            eprintln!("❌ Parsing failed: {:?}", e);
        }
    }
}
