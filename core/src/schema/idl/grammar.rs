// Comline IDL Grammar using rust-sitter

#[rust_sitter::grammar("idl")]
pub mod grammar {
    // Suppress dead code warnings for generated fields
    #![allow(dead_code)]
    
    // Whitespace and comment handling
    #[rust_sitter::extra]
    #[derive(Debug)]
    pub struct Whitespace(
        #[rust_sitter::leaf(pattern = r"\s+")]
        (),
    );
    
    #[rust_sitter::extra]
    #[derive(Debug)]
    pub struct Comment(
        #[rust_sitter::leaf(pattern = r"//[^\n]*")]
        (),
    );
    
    /// Language root - supports multiple declaration types  
    #[derive(Debug)]
    #[rust_sitter::language]
    pub enum Declaration {
        Import(Import),
        Const(Const),
        Struct(Struct),
        Enum(Enum),
        Protocol(Protocol),
    }
    
    // ===== Imports & Constants =====
    
    /// Import: import identifier
    #[derive(Debug)]
    pub struct Import(
        #[rust_sitter::leaf(text = "import")]
        (),
        
        Identifier,
    );
    
    /// Constant: const NAME: TYPE = VALUE
    #[derive(Debug)]
    pub struct Const(
        #[rust_sitter::leaf(text = "const")]
        (),
        
        Identifier,
        
        #[rust_sitter::leaf(text = ":")]
        (),
        
        Type,
        
        #[rust_sitter::leaf(text = "=")]
        (),
        
        Expression,
    );
    
    // ===== Struct Definition =====
    
    /// Struct: struct NAME { fields }
    #[derive(Debug)]
    pub struct Struct(
        #[rust_sitter::leaf(text = "struct")]
        (),
        
        Identifier,
        
        #[rust_sitter::leaf(text = "{")]
        (),
        
        #[rust_sitter::repeat(non_empty = false)]
        Vec<Field>,
        
        #[rust_sitter::leaf(text = "}")]
        (),
    );
    
    /// Field: name: Type
    #[derive(Debug)]
    pub struct Field(
        Identifier,
        
        #[rust_sitter::leaf(text = ":")]
        (),
        
        Type,
    );
    
    // ===== Enum Definition =====
    
    /// Enum: enum NAME { variants }
    #[derive(Debug)]
    pub struct Enum(
        #[rust_sitter::leaf(text = "enum")]
        (),
        
        Identifier,
        
        #[rust_sitter::leaf(text = "{")]
        (),
        
        #[rust_sitter::repeat(non_empty = true)]
        Vec<EnumVariant>,
        
        #[rust_sitter::leaf(text = "}")]
        (),
    );
    
    /// Enum variant: IDENTIFIER
    #[derive(Debug)]
    pub struct EnumVariant(Identifier);
    
    // ===== Protocol Definition =====
    
    /// Protocol: protocol NAME { functions }
    #[derive(Debug)]
    pub struct Protocol(
        #[rust_sitter::leaf(text = "protocol")]
        (),
        
        Identifier,
        
        #[rust_sitter::leaf(text = "{")]
        (),
        
        #[rust_sitter::repeat(non_empty = false)]
        Vec<Function>,
        
        #[rust_sitter::leaf(text = "}")]
        (),
    );
    
    /// Function: function NAME(args) returns Type
    #[derive(Debug)]
    pub struct Function(
        #[rust_sitter::leaf(text = "function")]
        (),
        
        Identifier,
        
        #[rust_sitter::leaf(text = "(")]
        (),
        
        #[rust_sitter::delimited(
            $(Argument),*
        )]
        Vec<Argument>,
        
        #[rust_sitter::leaf(text = ")")]
        (),
        
        #[rust_sitter::repeat(non_empty = false)]
        Option<ReturnType>,
    );
    
    /// Function argument (simplified)
    #[derive(Debug)]
    pub struct Argument(Type);
    
    /// Return type: returns Type
    #[derive(Debug)]
    pub struct ReturnType(
        #[rust_sitter::leaf(text = "returns")]
        (),
        
        Type,
    );
    
    // ===== Types =====
    
    /// Type
    #[derive(Debug)]
    pub enum Type {
        I8(I8Type),
        I16(I16Type),
        I32(I32Type),
        I64(I64Type),
        U8(U8Type),
        U16(U16Type),
        U32(U32Type),
        U64(U64Type),
        F32(F32Type),
        F64(F64Type),
        Bool(BoolType),
        Str(StrType),
        String(StringType),
        Named(Identifier),
    }
    
    #[derive(Debug)]
    #[rust_sitter::leaf(text = "i8")]
    pub struct I8Type;
    
    #[derive(Debug)]
    #[rust_sitter::leaf(text = "i16")]
    pub struct I16Type;
    
    #[derive(Debug)]
    #[rust_sitter::leaf(text = "i32")]
    pub struct I32Type;
    
    #[derive(Debug)]
    #[rust_sitter::leaf(text = "i64")]
    pub struct I64Type;
    
    #[derive(Debug)]
    #[rust_sitter::leaf(text = "u8")]
    pub struct U8Type;
    
    #[derive(Debug)]
    #[rust_sitter::leaf(text = "u16")]
    pub struct U16Type;
    
    #[derive(Debug)]
    #[rust_sitter::leaf(text = "u32")]
    pub struct U32Type;
    
    #[derive(Debug)]
    #[rust_sitter::leaf(text = "u64")]
    pub struct U64Type;
    
    #[derive(Debug)]
    #[rust_sitter::leaf(text = "f32")]
    pub struct F32Type;
    
    #[derive(Debug)]
    #[rust_sitter::leaf(text = "f64")]
    pub struct F64Type;
    
    #[derive(Debug)]
    #[rust_sitter::leaf(text = "bool")]
    pub struct BoolType;
    
    #[derive(Debug)]
    #[rust_sitter::leaf(text = "str")]
    pub struct StrType;
    
    #[derive(Debug)]
    #[rust_sitter::leaf(text = "string")]
    pub struct StringType;
    
    // ===== Expressions (Simplified) =====
    
    /// Expression (simplified for now)
    #[derive(Debug)]
    pub enum Expression {
        Integer(IntegerLiteral),
        String(StringLiteral),
        Identifier(Identifier),
    }
    
    #[derive(Debug)]
    pub struct IntegerLiteral(
        #[rust_sitter::leaf(pattern = r"\d+", transform = |s| s.parse().unwrap())]
        i64,
    );
    
    #[derive(Debug)]
    pub struct StringLiteral(
        #[rust_sitter::leaf(pattern = r#""([^"]*)""#, transform = |s| s[1..s.len()-1].to_string())]
        String,
    );
    
    /// Identifier: variable/type names
    #[derive(Debug)]
    pub struct Identifier(
        #[rust_sitter::leaf(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*", transform = |s| s.to_string())]
        String,
    );
}

// Re-export
pub use grammar::*;
