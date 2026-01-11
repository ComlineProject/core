// Comline IDL Grammar using rust-sitter

#[rust_sitter::grammar("idl")]
pub mod grammar {
    // Suppress dead code warnings for generated fields
    #![allow(dead_code)]

    // Whitespace and comment handling
    #[rust_sitter::extra]
    #[derive(Debug)]
    pub struct Whitespace(#[rust_sitter::leaf(pattern = r"\s+")] ());

    #[rust_sitter::extra]
    #[derive(Debug)]
    pub struct Comment(
        #[rust_sitter::leaf(pattern = r"//[^\n]*")]
        (),
    );

    /// Document root - supports multiple declarations
    #[derive(Debug)]
    #[rust_sitter::language]
    pub struct Document(#[rust_sitter::repeat(non_empty = false)] pub Vec<Declaration>);

    /// Language declarations - different statement types  
    #[derive(Debug, Clone)]
    pub enum Declaration {
        Import(Import),  // Legacy - kept for compatibility
        Use(Use),        // New: use keyword with enhanced features
        Const(Const),
        Struct(Struct),
        Enum(Enum),
        Protocol(Protocol),
    }

    // ===== Imports & Constants =====

    /// Import: import identifier (Legacy - for backward compatibility)
    #[derive(Debug, Clone)]
    pub struct Import {
        #[rust_sitter::leaf(text = "import")]
        _import: (),
        pub path: ScopedIdentifier,
    }

    // ===== Use Statements (New Import System) =====

    /// Use: use path [as alias]
    #[derive(Debug, Clone)]
    pub struct Use {
        #[rust_sitter::leaf(text = "use")]
        _use: (),
        pub path: UsePath,
        pub alias: Option<UseAlias>,
    }

    /// Use path - can be absolute, relative, glob, or multi-import
    #[derive(Debug, Clone)]
    pub enum UsePath {
        Absolute(ScopedIdentifier),
        Relative(RelativePath),
        Glob(GlobPath),
        Multi(MultiPath),
    }

    /// Relative path: self::path or parent::path
    #[derive(Debug, Clone)]
    pub struct RelativePath {
        pub prefix: RelativePrefix,
        #[rust_sitter::leaf(text = "::")]
        _sep: (),
        pub path: ScopedIdentifier,
    }

    /// Relative prefix: self, parent, crate
    #[derive(Debug, Clone)]
    pub enum RelativePrefix {
        #[rust_sitter::leaf(text = "self")]
        Self_,
        #[rust_sitter::leaf(text = "parent")]
        Parent,
        #[rust_sitter::leaf(text = "crate")]
        Crate,
    }

    /// Glob path: mypackage::types::*
    #[derive(Debug, Clone)]
    pub struct GlobPath {
        pub path: ScopedIdentifier,
        #[rust_sitter::leaf(text = "::")]
        _sep: (),
        #[rust_sitter::leaf(text = "*")]
        _star: (),
    }

    /// Multi-import: mypackage::{User, Post, Comment}
    #[derive(Debug, Clone)]
    pub struct MultiPath {
        pub path: ScopedIdentifier,
        #[rust_sitter::leaf(text = "::")]
        _sep: (),
        #[rust_sitter::leaf(text = "{")]
        _open: (),
        #[rust_sitter::delimited(
            #[rust_sitter::leaf(text = ",")]
            ()
        )]
        pub items: Vec<Identifier>,
        #[rust_sitter::leaf(text = "}")]
        _close: (),
    }

    /// Use alias: as NewName
    #[derive(Debug, Clone)]
    pub struct UseAlias {
        #[rust_sitter::leaf(text = "as")]
        _as: (),
        pub name: Identifier,
    }

    /// Constant: const NAME: TYPE = VALUE
    #[derive(Debug, Clone)]
    pub struct Const {
        #[rust_sitter::leaf(text = "const")]
        _const: (),
        pub name: Identifier,
        #[rust_sitter::leaf(text = ":")]
        _colon: (),
        pub type_def: Type,
        #[rust_sitter::leaf(text = "=")]
        _eq: (),
        pub value: Expression,
    }

    // ===== Struct Definition =====

    /// Struct: struct NAME { fields }
    #[derive(Debug, Clone)]
    pub struct Struct {
        #[rust_sitter::leaf(text = "struct")]
        _struct: (),
        pub name: Identifier,
        #[rust_sitter::leaf(text = "{")]
        _open: (),
        #[rust_sitter::repeat(non_empty = false)]
        pub fields: Vec<Field>,
        #[rust_sitter::leaf(text = "}")]
        _close: (),
    }

    /// Field: name: Type
    #[derive(Debug, Clone)]
    pub struct Field {
        #[rust_sitter::leaf(text = "optional")]
        pub optional: Option<()>,
        pub name: Identifier,
        #[rust_sitter::leaf(text = ":")]
        _colon: (),
        pub field_type: Type,
    }

    // ===== Enum Definition =====

    /// Enum: enum NAME { variants }
    #[derive(Debug, Clone)]
    pub struct Enum {
        #[rust_sitter::leaf(text = "enum")]
        _enum: (),
        pub name: Identifier,
        #[rust_sitter::leaf(text = "{")]
        _open: (),
        #[rust_sitter::repeat(non_empty = true)]
        pub variants: Vec<EnumVariant>,
        #[rust_sitter::leaf(text = "}")]
        _close: (),
    }

    /// Enum variant: IDENTIFIER
    #[derive(Debug, Clone)]
    pub struct EnumVariant {
        pub name: Identifier,
    }

    // ===== Protocol Definition =====

    // ===== Annotation Definition =====
    #[derive(Debug, Clone)]
    pub struct Annotation {
        #[rust_sitter::leaf(text = "@")]
        _at: (),
        pub key: Identifier,
        #[rust_sitter::leaf(text = "=")]
        _eq: (),
        pub value: Expression,
    }

    /// Protocol: protocol NAME { functions }
    #[derive(Debug, Clone)]
    pub struct Protocol {
        #[rust_sitter::repeat(non_empty = false)]
        pub annotations: Vec<Annotation>,
        #[rust_sitter::leaf(text = "protocol")]
        _protocol: (),
        pub name: Identifier,
        #[rust_sitter::leaf(text = "{")]
        _open: (),
        #[rust_sitter::repeat(non_empty = false)]
        pub functions: Vec<Function>,
        #[rust_sitter::leaf(text = "}")]
        _close: (),
    }

    /// Function: function NAME(args) returns Type
    #[derive(Debug, Clone)]
    pub struct Function {
        #[rust_sitter::repeat(non_empty = false)]
        pub annotations: Vec<Annotation>,
        #[rust_sitter::leaf(text = "function")]
        _fn: (),
        pub name: Identifier,
        #[rust_sitter::leaf(text = "(")]
        _open: (),
        #[rust_sitter::repeat(non_empty = false)]
        pub args: Option<ArgumentList>,
        #[rust_sitter::leaf(text = ")")]
        _close: (),
        #[rust_sitter::repeat(non_empty = false)]
        pub return_type: Option<ReturnType>,
        #[rust_sitter::leaf(text = ";")]
        _semi: (),
    }

    /// Argument list: first arg, then (comma + arg)*
    #[derive(Debug, Clone)]
    pub struct ArgumentList {
        pub first: Argument,
        #[rust_sitter::repeat(non_empty = false)]
        pub rest: Vec<CommaArgument>,
    }

    /// Comma followed by an argument
    #[derive(Debug, Clone)]
    pub struct CommaArgument {
        #[rust_sitter::leaf(text = ",")]
        _comma: (),
        pub arg: Argument,
    }

    /// Function argument (simplified) - just a type for now
    #[derive(Debug, Clone)]
    pub struct Argument {
        pub arg_type: Type,
    }

    /// Return type: returns Type
    #[derive(Debug, Clone)]
    pub struct ReturnType {
        #[rust_sitter::leaf(text = "->")]
        _arrow: (),
        pub return_type: Type,
    }

    // ===== Types =====

    /// Type
    #[derive(Debug, Clone)]
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
        Named(ScopedIdentifier),
        Array(Box<ArrayType>),
    }

    /// Array type: Type[] or Type[SIZE]
    #[derive(Debug, Clone)]
    pub struct ArrayType {
        pub key: Type,
        #[rust_sitter::leaf(text = "[")]
        _open: (),
        #[rust_sitter::repeat(non_empty = false)]
        pub size: Option<IntegerLiteral>,
        #[rust_sitter::leaf(text = "]")]
        _close: (),
    }

    #[derive(Debug, Clone)]
    #[rust_sitter::leaf(text = "i8")]
    pub struct I8Type;

    #[derive(Debug, Clone)]
    #[rust_sitter::leaf(text = "i16")]
    pub struct I16Type;

    #[derive(Debug, Clone)]
    #[rust_sitter::leaf(text = "i32")]
    pub struct I32Type;

    #[derive(Debug, Clone)]
    #[rust_sitter::leaf(text = "i64")]
    pub struct I64Type;

    #[derive(Debug, Clone)]
    #[rust_sitter::leaf(text = "u8")]
    pub struct U8Type;

    #[derive(Debug, Clone)]
    #[rust_sitter::leaf(text = "u16")]
    pub struct U16Type;

    #[derive(Debug, Clone)]
    #[rust_sitter::leaf(text = "u32")]
    pub struct U32Type;

    #[derive(Debug, Clone)]
    #[rust_sitter::leaf(text = "u64")]
    pub struct U64Type;

    #[derive(Debug, Clone)]
    #[rust_sitter::leaf(text = "f32")]
    pub struct F32Type;

    #[derive(Debug, Clone)]
    #[rust_sitter::leaf(text = "f64")]
    pub struct F64Type;

    #[derive(Debug, Clone)]
    #[rust_sitter::leaf(text = "bool")]
    pub struct BoolType;

    #[derive(Debug, Clone)]
    #[rust_sitter::leaf(text = "str")]
    pub struct StrType;

    #[derive(Debug, Clone)]
    #[rust_sitter::leaf(text = "string")]
    pub struct StringType;

    // ===== Expressions (Simplified) =====

    /// Expression (simplified for now)
    #[derive(Debug, Clone)]
    pub enum Expression {
        Integer(IntegerLiteral),
        String(StringLiteral),
        Identifier(Identifier),
    }

    #[derive(Debug, Clone)]
    pub struct IntegerLiteral {
        #[rust_sitter::leaf(pattern = r"-?\d+", transform = |s| s.parse().unwrap())]
        pub value: i64,
    }

    #[derive(Debug, Clone)]
    pub struct StringLiteral {
        #[rust_sitter::leaf(pattern = r#""([^"]*)""#, transform = |s| s[1..s.len()-1].to_string())]
        pub value: String,
    }

    /// Simple Identifier: variable/type names (no ::)
    #[derive(Debug, Clone)]
    pub struct Identifier {
        #[rust_sitter::leaf(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*", transform = |s| s.to_string())]
        pub text: String,
    }

    /// Scoped Identifier: paths with :: (e.g. package::module::Type)
    #[derive(Debug, Clone)]
    pub struct ScopedIdentifier {
        #[rust_sitter::leaf(pattern = r"[a-zA-Z_][a-zA-Z0-9_]*(::[a-zA-Z_][a-zA-Z0-9_]*)*", transform = |s| s.to_string())]
        pub text: String,
    }

    // Accessor methods for grammar types
    impl Import {
        pub fn path(&self) -> String {
            self.path.text.clone()
        }
    }

    impl Const {
        pub fn name(&self) -> String {
            self.name.text.clone()
        }
        pub fn type_def(&self) -> &Type {
            &self.type_def
        }
        pub fn value(&self) -> &Expression {
            &self.value
        }
    }

    impl Struct {
        pub fn name(&self) -> String {
            self.name.text.clone()
        }
        pub fn fields(&self) -> &Vec<Field> {
            &self.fields
        }
    }

    impl Field {
        pub fn optional(&self) -> bool {
            self.optional.is_some()
        }
        pub fn name(&self) -> String {
            self.name.text.clone()
        }
        pub fn field_type(&self) -> &Type {
            &self.field_type
        }
    }

    impl Enum {
        pub fn name(&self) -> String {
            self.name.text.clone()
        }
        pub fn variants(&self) -> &Vec<EnumVariant> {
            &self.variants
        }
    }

    impl Protocol {
        pub fn annotations(&self) -> &Vec<Annotation> {
            &self.annotations
        }
        pub fn name(&self) -> String {
            self.name.text.clone()
        }
        pub fn functions(&self) -> &Vec<Function> {
            &self.functions
        }
    }

    impl Function {
        pub fn annotations(&self) -> &Vec<Annotation> {
            &self.annotations
        }
        pub fn name(&self) -> String {
            self.name.text.clone()
        }
        pub fn args(&self) -> &Option<ArgumentList> {
            &self.args
        }
        pub fn return_type(&self) -> &Option<ReturnType> {
            &self.return_type
        }
    }

    impl ArgumentList {
        pub fn first(&self) -> &Argument {
            &self.first
        }
        pub fn rest(&self) -> &Vec<CommaArgument> {
            &self.rest
        }
    }

    impl CommaArgument {
        pub fn arg_type(&self) -> &Argument {
            &self.arg
        }
    }

    impl Identifier {
        pub fn as_str(&self) -> &str {
            &self.text
        }
        pub fn to_string(&self) -> String {
            self.text.clone()
        }
    }

    impl IntegerLiteral {
        pub fn value(&self) -> i64 {
            self.value
        }
    }

    impl StringLiteral {
        pub fn value(&self) -> &str {
            &self.value
        }
    }

    impl ArrayType {
        pub fn elem_type(&self) -> &Type {
            &self.key
        }
    }

    impl EnumVariant {
        pub fn identifier(&self) -> &Identifier {
            &self.name
        }
    }

    impl Argument {
        pub fn arg_type(&self) -> &Type {
            &self.arg_type
        }
    }

    impl ReturnType {
        pub fn return_type(&self) -> &Type {
            &self.return_type
        }
    }

    impl ScopedIdentifier {
        pub fn as_str(&self) -> &str {
            &self.text
        }
        pub fn to_string(&self) -> String {
            self.text.clone()
        }
    }

    impl Annotation {
        pub fn key(&self) -> String {
            self.key.text.clone()
        }
        pub fn value(&self) -> String {
            match &self.value {
                Expression::Integer(i) => i.value.to_string(),
                Expression::String(s) => s.value.clone(),
                Expression::Identifier(i) => i.text.clone(),
            }
        }
    }
}

// Re-export
pub use grammar::*;
