// Test: Parse IDL code using rust-sitter parser

fn main() {
    let idl_code = r#"
struct Message {
    sender: str
    content: str
}
"#;

    println!("Testing rust-sitter parser...\n");
    println!("IDL Code:\n{}\n", idl_code);
    println!("Attempting to parse...\n");
    
    // Parse with rust-sitter
    match comline_core::schema::idl::grammar::parse(idl_code) {
        Ok(declaration) => {
            use comline_core::schema::idl::grammar::Declaration;
            
            println!("✅ Parse successful!");
            println!("Received declaration type: {}", 
                match &declaration {
                    Declaration::Import(_) => "Import",
                    Declaration::Const(_) => "Const",
                    Declaration::Struct(_) => "Struct",
                    Declaration::Enum(_) => "Enum",
                    Declaration::Protocol(_) => "Protocol",
                }
            );
            
            // Extract data using accessor methods
            if let Declaration::Struct(s) = declaration {
                println!("\nStruct name: {}", s.get_name());
                println!("Fields:");
                for (name, typ) in s.get_fields() {
                    println!("  {}: {}", name, typ);
                }
            }
        }
        Err(e) => {
            println!("❌ Parse error: {:?}", e);
        }
    }
}
