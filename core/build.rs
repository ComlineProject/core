fn main() {
    // Compile the rust-sitter grammars. Only the `compiler` feature has a
    // parser; the IR-only build has nothing to generate.
    #[cfg(feature = "compiler")]
    {
        use rust_sitter_tool::build_parsers;
        use std::path::Path;

        build_parsers(Path::new("src/schema/idl/grammar.rs"));
        build_parsers(Path::new("src/package/config/idl/grammar.rs"));
    }

    // Tell Cargo to rerun if grammar changes
    println!("cargo:rerun-if-changed=src/schema/idl/grammar.rs");
    println!("cargo:rerun-if-changed=src/package/config/idl/grammar.rs");
}
