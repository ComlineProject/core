// Comline IDL Grammar using rust-sitter

#[rust_sitter::grammar("idl")]
pub mod grammar {
    // Suppress dead code warnings for generated fields
    #![allow(dead_code)]

    // Whitespace and comment handling
    #[rust_sitter::extra]
    #[derive(Debug)]
    pub struct Whitespace(#[rust_sitter::leaf(pattern = r"\s+")] ());

    // A `//` comment, but never one starting with a third `/` - `///`
    // lines are docstrings (see `Docstring` below), a real grammar rule
    // rather than a discarded `extra`, and need to win the lexer's
    // longest-match comparison against this rule wherever both could
    // apply. Restricting this pattern to at most 2 characters on `///`
    // input (instead of tying with Docstring's full-line match) resolves
    // that unambiguously, rather than relying on an unspecified tie-break.
    #[rust_sitter::extra]
    #[derive(Debug)]
    pub struct Comment(
        #[rust_sitter::leaf(pattern = r"//[^/\n][^\n]*|//")]
        (),
    );

    /// Document root - supports multiple declarations
    #[derive(Debug)]
    #[rust_sitter::language]
    pub struct Document(#[rust_sitter::repeat(non_empty = false)] pub Vec<rust_sitter::Spanned<Declaration>>);

    /// Language declarations - different statement types  
    #[derive(Debug, Clone)]
    pub enum Declaration {
        Import(Import),  // Legacy - kept for compatibility
        Use(Use),        // New: use keyword with enhanced features
        Const(Const),
        Struct(Struct),
        Enum(Enum),
        Protocol(Protocol),
        Error(Error),
        Settings(Settings),
        Validator(Validator),
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
        #[rust_sitter::repeat(non_empty = false)]
        pub docstring: Option<Docstring>,
        #[rust_sitter::leaf(text = "const")]
        _const: (),
        pub name: Identifier,
        #[rust_sitter::leaf(text = ":")]
        _colon: (),
        pub type_def: rust_sitter::Spanned<Type>,
        #[rust_sitter::leaf(text = "=")]
        _eq: (),
        pub value: Expression,
    }

    // ===== Struct Definition =====

    /// Struct: struct NAME { fields }
    #[derive(Debug, Clone)]
    pub struct Struct {
        #[rust_sitter::repeat(non_empty = false)]
        pub docstring: Option<Docstring>,
        #[rust_sitter::repeat(non_empty = false)]
        pub annotations: Option<Annotations>,
        #[rust_sitter::leaf(text = "struct")]
        _struct: (),
        pub name: Identifier,
        #[rust_sitter::leaf(text = "{")]
        _open: (),
        #[rust_sitter::repeat(non_empty = false)]
        pub fields: Vec<rust_sitter::Spanned<Field>>,
        #[rust_sitter::leaf(text = "}")]
        _close: (),
    }

    /// Field: name: Type [= default]
    #[derive(Debug, Clone)]
    pub struct Field {
        #[rust_sitter::repeat(non_empty = false)]
        pub docstring: Option<Docstring>,
        #[rust_sitter::repeat(non_empty = false)]
        pub annotations: Option<Annotations>,
        #[rust_sitter::leaf(text = "optional")]
        pub optional: Option<()>,
        pub name: Identifier,
        #[rust_sitter::leaf(text = ":")]
        _colon: (),
        pub field_type: rust_sitter::Spanned<Type>,
        #[rust_sitter::repeat(non_empty = false)]
        pub default: Option<FieldDefault>,
    }

    /// Default value for a struct field: = VALUE
    #[derive(Debug, Clone)]
    pub struct FieldDefault {
        #[rust_sitter::leaf(text = "=")]
        _eq: (),
        pub value: Expression,
    }

    // ===== Error Definition =====

    /// Error: error NAME { message = "..." <fields> }
    #[derive(Debug, Clone)]
    pub struct Error {
        #[rust_sitter::repeat(non_empty = false)]
        pub docstring: Option<Docstring>,
        #[rust_sitter::leaf(text = "error")]
        _error: (),
        pub name: Identifier,
        #[rust_sitter::leaf(text = "{")]
        _open: (),
        pub message: ErrorMessage,
        #[rust_sitter::repeat(non_empty = false)]
        pub fields: Vec<rust_sitter::Spanned<Field>>,
        #[rust_sitter::leaf(text = "}")]
        _close: (),
    }

    /// message = "...{placeholder}..." - always required, every real
    /// example has one.
    #[derive(Debug, Clone)]
    pub struct ErrorMessage {
        #[rust_sitter::leaf(text = "message")]
        _message: (),
        #[rust_sitter::leaf(text = "=")]
        _eq: (),
        pub value: InterpolatedString,
    }

    /// An interpolated string: `{path}` substitutes a dotted-path value;
    /// a doubled brace (`{{` / `}}`) is an escaped literal `{`/`}`; any
    /// other character is ordinary literal text. Scoped narrowly to
    /// `message = ...` - not the general `StringLiteral` used by
    /// `const`/annotation/field-default values, which stay plain and
    /// non-interpolating.
    #[derive(Debug, Clone)]
    pub struct InterpolatedString {
        #[rust_sitter::leaf(text = "\"")]
        _open: (),
        #[rust_sitter::repeat(non_empty = false)]
        pub parts: Vec<InterpolatedPart>,
        #[rust_sitter::leaf(text = "\"")]
        _close: (),
    }

    #[derive(Debug, Clone)]
    pub enum InterpolatedPart {
        Text(InterpolatedText),
        Placeholder(InterpolatedPlaceholder),
        EscapedOpenBrace(EscapedOpenBrace),
        EscapedCloseBrace(EscapedCloseBrace),
    }

    /// A run of plain characters between braces/quotes - braces are
    /// excluded here so `{`/`}`/`{{`/`}}` are always handled by the other
    /// `InterpolatedPart` alternatives instead of being silently absorbed
    /// as ordinary text.
    #[derive(Debug, Clone)]
    pub struct InterpolatedText {
        #[rust_sitter::leaf(pattern = r#"[^"{}]+"#, transform = |s| s.to_string())]
        pub text: String,
    }

    #[derive(Debug, Clone)]
    pub struct InterpolatedPlaceholder {
        #[rust_sitter::leaf(text = "{")]
        _open: (),
        pub path: DottedPath,
        #[rust_sitter::leaf(text = "}")]
        _close: (),
    }

    /// `{{` -> a single literal `{`. A 2-character leaf always wins
    /// tree-sitter's longest-match comparison against `InterpolatedPlaceholder`'s
    /// 1-character opening `{` at the same position, so there's no
    /// ambiguity between "start of an escape" and "start of a
    /// placeholder" - and even without that preference, treating the
    /// second `{` as the start of a `DottedPath` would fail immediately
    /// (`{` isn't a valid identifier start), so GLR would prune that
    /// reading anyway.
    #[derive(Debug, Clone)]
    pub struct EscapedOpenBrace(#[rust_sitter::leaf(text = "{{")] ());

    /// `}}` -> a single literal `}` - same reasoning as `EscapedOpenBrace`.
    #[derive(Debug, Clone)]
    pub struct EscapedCloseBrace(#[rust_sitter::leaf(text = "}}")] ());

    /// A dotted path like `self.recipient`, used only inside interpolated
    /// -string placeholders - not a general-purpose expression, and not
    /// reused for throws-argument-binding (out of scope for now).
    #[derive(Debug, Clone)]
    pub struct DottedPath {
        pub first: Identifier,
        #[rust_sitter::repeat(non_empty = false)]
        pub rest: Vec<DottedPathSegment>,
    }

    #[derive(Debug, Clone)]
    pub struct DottedPathSegment {
        #[rust_sitter::leaf(text = ".")]
        _dot: (),
        pub segment: Identifier,
    }

    /// `! ErrorName` on a function - bare reference only, no
    /// argument-binding into the error's fields (out of scope for now).
    #[derive(Debug, Clone)]
    pub struct Throws {
        #[rust_sitter::leaf(text = "!")]
        _bang: (),
        pub error_name: Identifier,
    }

    // ===== Enum Definition =====

    /// Enum: enum NAME { variants }
    #[derive(Debug, Clone)]
    pub struct Enum {
        #[rust_sitter::repeat(non_empty = false)]
        pub docstring: Option<Docstring>,
        #[rust_sitter::leaf(text = "enum")]
        _enum: (),
        pub name: Identifier,
        #[rust_sitter::leaf(text = "{")]
        _open: (),
        #[rust_sitter::repeat(non_empty = true)]
        pub variants: Vec<rust_sitter::Spanned<EnumVariant>>,
        #[rust_sitter::leaf(text = "}")]
        _close: (),
    }

    /// Enum variant: IDENTIFIER
    #[derive(Debug, Clone)]
    pub struct EnumVariant {
        pub name: Identifier,
    }

    // ===== Settings Definition =====

    /// Settings: settings NAME { key = value ... }
    ///
    /// Schema-wide switches. Each value is a boolean, integer or string - there
    /// is no expression language here. Parsed and frozen today; not yet
    /// enforced.
    #[derive(Debug, Clone)]
    pub struct Settings {
        #[rust_sitter::repeat(non_empty = false)]
        pub docstring: Option<Docstring>,
        #[rust_sitter::leaf(text = "settings")]
        _settings: (),
        pub name: Identifier,
        #[rust_sitter::leaf(text = "{")]
        _open: (),
        #[rust_sitter::repeat(non_empty = false)]
        pub entries: Vec<rust_sitter::Spanned<Setting>>,
        #[rust_sitter::leaf(text = "}")]
        _close: (),
    }

    /// One `key = value` line inside a `settings` block.
    #[derive(Debug, Clone)]
    pub struct Setting {
        pub key: Identifier,
        #[rust_sitter::leaf(text = "=")]
        _eq: (),
        pub value: SettingValue,
    }

    /// A `settings` value. No identifier alternative, so `True` / `False` lex
    /// unambiguously here (unlike in a general `Expression`).
    #[derive(Debug, Clone)]
    pub enum SettingValue {
        Bool(BoolLiteral),
        Integer(IntegerLiteral),
        Str(StringLiteral),
    }

    #[derive(Debug, Clone)]
    pub enum BoolLiteral {
        #[rust_sitter::leaf(text = "True")]
        True,
        #[rust_sitter::leaf(text = "False")]
        False,
    }

    // ===== Validator Definition =====

    /// Validator: validator NAME { property* }
    ///
    /// A named, parameterised field check. Phase 1: the declaration and its
    /// typed properties parse and freeze. The `validate { ... }` block (the
    /// assert expression language) is not in the grammar yet - see the
    /// validators design note.
    #[derive(Debug, Clone)]
    pub struct Validator {
        #[rust_sitter::repeat(non_empty = false)]
        pub docstring: Option<Docstring>,
        #[rust_sitter::leaf(text = "validator")]
        _validator: (),
        pub name: Identifier,
        #[rust_sitter::leaf(text = "{")]
        _open: (),
        #[rust_sitter::repeat(non_empty = false)]
        pub properties: Vec<rust_sitter::Spanned<ValidatorProperty>>,
        #[rust_sitter::repeat(non_empty = false)]
        pub validate: Option<ValidateBlock>,
        #[rust_sitter::leaf(text = "}")]
        _close: (),
    }

    /// `name: Type [= default]` - a validator's configuration parameter. Same
    /// shape as a struct field, minus `optional` and annotations.
    #[derive(Debug, Clone)]
    pub struct ValidatorProperty {
        #[rust_sitter::repeat(non_empty = false)]
        pub docstring: Option<Docstring>,
        pub name: Identifier,
        #[rust_sitter::leaf(text = ":")]
        _colon: (),
        pub property_type: rust_sitter::Spanned<Type>,
        #[rust_sitter::repeat(non_empty = false)]
        pub default: Option<FieldDefault>,
    }

    /// `validate { assert(...) ... }`. The assert conditions use a deliberately
    /// small expression language - member access, comparisons, `and` / `or` -
    /// captured as text, not evaluated (see the validators design note).
    #[derive(Debug, Clone)]
    pub struct ValidateBlock {
        #[rust_sitter::leaf(text = "validate")]
        _validate: (),
        #[rust_sitter::leaf(text = "{")]
        _open: (),
        #[rust_sitter::repeat(non_empty = false)]
        pub asserts: Vec<rust_sitter::Spanned<AssertCall>>,
        #[rust_sitter::leaf(text = "}")]
        _close: (),
    }

    /// `assert(<condition>, <message>)`
    #[derive(Debug, Clone)]
    pub struct AssertCall {
        #[rust_sitter::leaf(text = "assert")]
        _assert: (),
        #[rust_sitter::leaf(text = "(")]
        _open: (),
        pub condition: Condition,
        #[rust_sitter::leaf(text = ",")]
        _comma: (),
        pub message: InterpolatedString,
        #[rust_sitter::leaf(text = ")")]
        _close: (),
    }

    /// One or more comparisons joined by `and` / `or`. Precedence is not
    /// modelled - the block is text, not an evaluated AST.
    #[derive(Debug, Clone)]
    pub struct Condition {
        pub first: Comparison,
        #[rust_sitter::repeat(non_empty = false)]
        pub rest: Vec<ConditionTail>,
    }

    #[derive(Debug, Clone)]
    pub struct ConditionTail {
        pub op: LogicalOp,
        pub comparison: Comparison,
    }

    #[derive(Debug, Clone)]
    pub enum LogicalOp {
        #[rust_sitter::leaf(text = "and")]
        And,
        #[rust_sitter::leaf(text = "or")]
        Or,
    }

    #[derive(Debug, Clone)]
    pub struct Comparison {
        pub left: Operand,
        pub op: CompareOp,
        pub right: Operand,
    }

    #[derive(Debug, Clone)]
    pub enum CompareOp {
        #[rust_sitter::leaf(text = "==")]
        Eq,
        #[rust_sitter::leaf(text = "!=")]
        Ne,
        #[rust_sitter::leaf(text = ">=")]
        Ge,
        #[rust_sitter::leaf(text = "<=")]
        Le,
        #[rust_sitter::leaf(text = ">")]
        Gt,
        #[rust_sitter::leaf(text = "<")]
        Lt,
    }

    /// A value in a comparison: a member path (`value.length`,
    /// `params.min_chars`, or a bare name), an integer, or a plain string.
    #[derive(Debug, Clone)]
    pub enum Operand {
        Path(DottedPath),
        Integer(IntegerLiteral),
        Str(StringLiteral),
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
        pub value: AnnotationValue,
    }

    /// An annotation's value: a scalar (`@timeout_ms = 1000`), or a list of
    /// calls (`@validators = [StringBounds(min_chars = 3, max_chars = 12)]`).
    #[derive(Debug, Clone)]
    pub enum AnnotationValue {
        Scalar(Expression),
        List(AnnotationList),
    }

    /// `[Call(args), Call(args), ...]`
    #[derive(Debug, Clone)]
    pub struct AnnotationList {
        #[rust_sitter::leaf(text = "[")]
        _open: (),
        #[rust_sitter::delimited(
            #[rust_sitter::leaf(text = ",")]
            ()
        )]
        pub items: Vec<AnnotationCall>,
        #[rust_sitter::leaf(text = "]")]
        _close: (),
    }

    /// `Name(key = value, key = value)` - a named call with keyword arguments.
    #[derive(Debug, Clone)]
    pub struct AnnotationCall {
        pub name: Identifier,
        #[rust_sitter::leaf(text = "(")]
        _open: (),
        #[rust_sitter::delimited(
            #[rust_sitter::leaf(text = ",")]
            ()
        )]
        pub args: Vec<AnnotationArg>,
        #[rust_sitter::leaf(text = ")")]
        _close: (),
    }

    #[derive(Debug, Clone)]
    pub struct AnnotationArg {
        pub name: Identifier,
        #[rust_sitter::leaf(text = "=")]
        _eq: (),
        pub value: Expression,
    }

    /// A non-empty list of `@key=value` annotations, as a single named
    /// grammar rule shared by every declaration kind that can carry them
    /// (`Struct`, `Field`, `Protocol`, `Function`), always used behind
    /// `Option<Annotations>` (`None` = zero annotations) rather than
    /// letting this rule itself expand to nothing. Each of those used to
    /// declare its own independent, inline `Vec<Annotation>` repeat, which
    /// tree-sitter's grammar generator couldn't disambiguate between at
    /// parse time (it can't tell, from an `Annotation` alone, which
    /// parent's list it belongs to when several appear back to back before
    /// the next declaration) - one shared rule referenced by all four
    /// avoids the ambiguity entirely instead of resolving it.
    #[derive(Debug, Clone)]
    pub struct Annotations {
        pub first: Annotation,
        #[rust_sitter::repeat(non_empty = false)]
        pub rest: Vec<Annotation>,
    }

    // ===== Docstring Definition =====

    /// One `///` line of documentation - either a plain description line
    /// or an `/// @name: description` line documenting one field/argument.
    /// Both forms are captured as raw text (the `///` marker and one
    /// following space stripped); the `@name:` form isn't parsed out into
    /// structured data - see `Docstring` below for why.
    #[derive(Debug, Clone)]
    pub struct DocLine {
        #[rust_sitter::leaf(
            pattern = r"///[^\n]*",
            transform = |s| s.trim_start_matches('/').trim_start().to_string()
        )]
        pub text: String,
    }

    /// A non-empty block of consecutive `///` lines, as a single named
    /// grammar rule shared by every declaration kind that can carry one
    /// (`Const`, `Struct`, `Field`, `Enum`, `Protocol`, `Function`), always
    /// used behind `Option<Docstring>` (`None` = no docstring) - same
    /// reasoning as `Annotations` above: one shared rule referenced by six
    /// parents avoids a tree-sitter parse-table conflict that six
    /// independently-declared identical-shaped repeats would very likely
    /// hit.
    #[derive(Debug, Clone)]
    pub struct Docstring {
        pub first: DocLine,
        #[rust_sitter::repeat(non_empty = false)]
        pub rest: Vec<DocLine>,
    }

    /// Protocol: protocol NAME { functions }
    #[derive(Debug, Clone)]
    pub struct Protocol {
        #[rust_sitter::repeat(non_empty = false)]
        pub docstring: Option<Docstring>,
        #[rust_sitter::repeat(non_empty = false)]
        pub annotations: Option<Annotations>,
        #[rust_sitter::leaf(text = "protocol")]
        _protocol: (),
        pub name: Identifier,
        #[rust_sitter::leaf(text = "{")]
        _open: (),
        #[rust_sitter::repeat(non_empty = false)]
        pub functions: Vec<rust_sitter::Spanned<Function>>,
        #[rust_sitter::leaf(text = "}")]
        _close: (),
    }

    /// Function: function NAME(args) returns Type
    #[derive(Debug, Clone)]
    pub struct Function {
        #[rust_sitter::repeat(non_empty = false)]
        pub docstring: Option<Docstring>,
        #[rust_sitter::repeat(non_empty = false)]
        pub annotations: Option<Annotations>,
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
        #[rust_sitter::repeat(non_empty = false)]
        pub throws: Option<Throws>,
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

    /// Optional `name:` prefix on a function argument.
    #[derive(Debug, Clone)]
    pub struct ArgumentName {
        pub name: ScopedIdentifier,
        #[rust_sitter::leaf(text = ":")]
        _colon: (),
    }

    /// Function argument: an optional `name:` prefix, then the type -
    /// e.g. `string` (bare) or `value: string` (named).
    #[derive(Debug, Clone)]
    pub struct Argument {
        #[rust_sitter::repeat(non_empty = false)]
        pub name: Option<ArgumentName>,
        pub arg_type: rust_sitter::Spanned<Type>,
    }

    /// Return type: returns Type
    #[derive(Debug, Clone)]
    pub struct ReturnType {
        #[rust_sitter::leaf(text = "->")]
        _arrow: (),
        pub return_type: rust_sitter::Spanned<Type>,
    }

    // ===== Types =====

    /// Type
    #[derive(Debug, Clone)]
    pub enum Type {
        S8(S8Type),
        S16(S16Type),
        S32(S32Type),
        S64(S64Type),
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
        Union(UnionType),
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

    /// Union type: union(Type Type ...)
    #[derive(Debug, Clone)]
    pub struct UnionType {
        #[rust_sitter::leaf(text = "union")]
        _union: (),
        #[rust_sitter::leaf(text = "(")]
        _open: (),
        #[rust_sitter::repeat(non_empty = true)]
        pub members: Vec<Type>,
        #[rust_sitter::leaf(text = ")")]
        _close: (),
    }

    #[derive(Debug, Clone)]
    #[rust_sitter::leaf(text = "s8")]
    pub struct S8Type;

    #[derive(Debug, Clone)]
    #[rust_sitter::leaf(text = "s16")]
    pub struct S16Type;

    #[derive(Debug, Clone)]
    #[rust_sitter::leaf(text = "s32")]
    pub struct S32Type;

    #[derive(Debug, Clone)]
    #[rust_sitter::leaf(text = "s64")]
    pub struct S64Type;

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
        FString(FString),
        Path(ScopedPath),
        Identifier(Identifier),
    }

    /// A `::`-separated path used as a value: `u32::MIN`, `pkg::mod::DEFAULT`.
    /// Requires at least one `::` so it never overlaps a bare `Identifier`.
    /// The path is recorded as text; it is not resolved to a value yet.
    #[derive(Debug, Clone)]
    pub struct ScopedPath {
        #[rust_sitter::leaf(
            pattern = r"[a-zA-Z_][a-zA-Z0-9_]*(::[a-zA-Z_][a-zA-Z0-9_]*)+",
            transform = |s| s.to_string()
        )]
        pub text: String,
    }

    /// An f-string value: `f"flower power: {POWER}"`. Same interpolation as an
    /// `error` message - `{dotted.path}` placeholders, `{{` / `}}` escapes. The
    /// opening `f"` is a single token, so a bare identifier `f` is never
    /// mistaken for the start of one. Recorded as text; `{POWER}` is not
    /// resolved to the constant's value yet.
    #[derive(Debug, Clone)]
    pub struct FString {
        #[rust_sitter::leaf(text = "f\"")]
        _open: (),
        #[rust_sitter::repeat(non_empty = false)]
        pub parts: Vec<InterpolatedPart>,
        #[rust_sitter::leaf(text = "\"")]
        _close: (),
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
        pub fn docstring(&self) -> Option<String> {
            self.docstring.as_ref().map(|d| d.joined())
        }
        pub fn name(&self) -> String {
            self.name.text.clone()
        }
        pub fn type_def(&self) -> &Type {
            &self.type_def.value
        }
        pub fn type_def_span(&self) -> (usize, usize) {
            self.type_def.span
        }
        pub fn value(&self) -> &Expression {
            &self.value
        }
    }

    impl Struct {
        pub fn docstring(&self) -> Option<String> {
            self.docstring.as_ref().map(|d| d.joined())
        }
        pub fn annotations(&self) -> Vec<&Annotation> {
            self.annotations
                .as_ref()
                .map(|a| a.iter().collect())
                .unwrap_or_default()
        }
        pub fn name(&self) -> String {
            self.name.text.clone()
        }
        pub fn fields(&self) -> &Vec<rust_sitter::Spanned<Field>> {
            &self.fields
        }
    }

    impl Field {
        pub fn docstring(&self) -> Option<String> {
            self.docstring.as_ref().map(|d| d.joined())
        }
        pub fn annotations(&self) -> Vec<&Annotation> {
            self.annotations
                .as_ref()
                .map(|a| a.iter().collect())
                .unwrap_or_default()
        }
        pub fn optional(&self) -> bool {
            self.optional.is_some()
        }
        pub fn name(&self) -> String {
            self.name.text.clone()
        }
        pub fn field_type(&self) -> &Type {
            &self.field_type.value
        }
        pub fn field_type_span(&self) -> (usize, usize) {
            self.field_type.span
        }
        pub fn default_value(&self) -> Option<&Expression> {
            self.default.as_ref().map(|d| &d.value)
        }
    }

    impl Error {
        pub fn docstring(&self) -> Option<String> {
            self.docstring.as_ref().map(|d| d.joined())
        }
        pub fn name(&self) -> String {
            self.name.text.clone()
        }
        pub fn message(&self) -> String {
            self.message.value.reconstruct()
        }
        pub fn fields(&self) -> &Vec<rust_sitter::Spanned<Field>> {
            &self.fields
        }
    }

    impl InterpolatedString {
        /// Rebuild the original template text: literal parts as-is,
        /// placeholders re-inserted as `{a.b.c}`, escaped braces
        /// re-inserted as the single literal character they represent.
        pub fn reconstruct(&self) -> String {
            render_interpolated_parts(&self.parts)
        }
    }

    impl FString {
        /// The template between the quotes: `flower power: {POWER}`.
        pub fn template(&self) -> String {
            render_interpolated_parts(&self.parts)
        }
        /// The full source form: `f"flower power: {POWER}"`.
        pub fn source(&self) -> String {
            format!("f\"{}\"", self.template())
        }
    }

    fn render_interpolated_parts(parts: &[InterpolatedPart]) -> String {
        let mut out = String::new();
        for part in parts {
            match part {
                InterpolatedPart::Text(t) => out.push_str(&t.text),
                InterpolatedPart::Placeholder(p) => {
                    out.push('{');
                    out.push_str(&p.path.joined());
                    out.push('}');
                }
                InterpolatedPart::EscapedOpenBrace(_) => out.push('{'),
                InterpolatedPart::EscapedCloseBrace(_) => out.push('}'),
            }
        }
        out
    }

    impl DottedPath {
        pub fn joined(&self) -> String {
            let mut segments = vec![self.first.text.clone()];
            segments.extend(self.rest.iter().map(|s| s.segment.text.clone()));
            segments.join(".")
        }
    }

    impl Enum {
        pub fn docstring(&self) -> Option<String> {
            self.docstring.as_ref().map(|d| d.joined())
        }
        pub fn name(&self) -> String {
            self.name.text.clone()
        }
        pub fn variants(&self) -> &Vec<rust_sitter::Spanned<EnumVariant>> {
            &self.variants
        }
    }

    impl Settings {
        pub fn docstring(&self) -> Option<String> {
            self.docstring.as_ref().map(|d| d.joined())
        }
        pub fn name(&self) -> String {
            self.name.text.clone()
        }
        pub fn entries(&self) -> &Vec<rust_sitter::Spanned<Setting>> {
            &self.entries
        }
    }

    impl Setting {
        pub fn key(&self) -> String {
            self.key.text.clone()
        }
        /// The value rendered as a plain string (`"True"`, `"8"`, `"core"`).
        pub fn value_string(&self) -> String {
            match &self.value {
                SettingValue::Bool(BoolLiteral::True) => "True".to_string(),
                SettingValue::Bool(BoolLiteral::False) => "False".to_string(),
                SettingValue::Integer(i) => i.value.to_string(),
                SettingValue::Str(s) => s.value.clone(),
            }
        }
    }

    impl Validator {
        pub fn docstring(&self) -> Option<String> {
            self.docstring.as_ref().map(|d| d.joined())
        }
        pub fn name(&self) -> String {
            self.name.text.clone()
        }
        pub fn properties(&self) -> &Vec<rust_sitter::Spanned<ValidatorProperty>> {
            &self.properties
        }
        /// Each `assert(...)` in the `validate` block, rebuilt as text. Empty
        /// when there is no `validate` block.
        pub fn validate_asserts(&self) -> Vec<String> {
            self.validate
                .as_ref()
                .map(|v| v.asserts.iter().map(|a| a.reconstruct()).collect())
                .unwrap_or_default()
        }
    }

    impl AssertCall {
        pub fn reconstruct(&self) -> String {
            format!(
                "assert({}, {:?})",
                self.condition.reconstruct(),
                self.message.reconstruct()
            )
        }
    }

    impl Condition {
        pub fn reconstruct(&self) -> String {
            let mut out = self.first.reconstruct();
            for tail in &self.rest {
                out.push_str(&format!(
                    " {} {}",
                    tail.op.as_str(),
                    tail.comparison.reconstruct()
                ));
            }
            out
        }
    }

    impl Comparison {
        pub fn reconstruct(&self) -> String {
            format!(
                "{} {} {}",
                self.left.reconstruct(),
                self.op.as_str(),
                self.right.reconstruct()
            )
        }
    }

    impl LogicalOp {
        pub fn as_str(&self) -> &'static str {
            match self {
                LogicalOp::And => "and",
                LogicalOp::Or => "or",
            }
        }
    }

    impl CompareOp {
        pub fn as_str(&self) -> &'static str {
            match self {
                CompareOp::Eq => "==",
                CompareOp::Ne => "!=",
                CompareOp::Ge => ">=",
                CompareOp::Le => "<=",
                CompareOp::Gt => ">",
                CompareOp::Lt => "<",
            }
        }
    }

    impl Operand {
        pub fn reconstruct(&self) -> String {
            match self {
                Operand::Path(p) => p.joined(),
                Operand::Integer(i) => i.value.to_string(),
                Operand::Str(s) => format!("{:?}", s.value),
            }
        }
    }

    impl ValidatorProperty {
        pub fn docstring(&self) -> Option<String> {
            self.docstring.as_ref().map(|d| d.joined())
        }
        pub fn name(&self) -> String {
            self.name.text.clone()
        }
        pub fn property_type(&self) -> &Type {
            &self.property_type.value
        }
        pub fn default_value(&self) -> Option<&Expression> {
            self.default.as_ref().map(|d| &d.value)
        }
    }

    impl Protocol {
        pub fn docstring(&self) -> Option<String> {
            self.docstring.as_ref().map(|d| d.joined())
        }
        pub fn annotations(&self) -> Vec<&Annotation> {
            self.annotations
                .as_ref()
                .map(|a| a.iter().collect())
                .unwrap_or_default()
        }
        pub fn name(&self) -> String {
            self.name.text.clone()
        }
        pub fn functions(&self) -> &Vec<rust_sitter::Spanned<Function>> {
            &self.functions
        }
    }

    impl Function {
        pub fn docstring(&self) -> Option<String> {
            self.docstring.as_ref().map(|d| d.joined())
        }
        pub fn annotations(&self) -> Vec<&Annotation> {
            self.annotations
                .as_ref()
                .map(|a| a.iter().collect())
                .unwrap_or_default()
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
        pub fn throws(&self) -> Option<String> {
            self.throws.as_ref().map(|t| t.error_name.text.clone())
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

    impl UnionType {
        pub fn members(&self) -> &Vec<Type> {
            &self.members
        }
    }

    impl EnumVariant {
        pub fn identifier(&self) -> &Identifier {
            &self.name
        }
    }

    impl Argument {
        pub fn arg_type(&self) -> &Type {
            &self.arg_type.value
        }
        pub fn arg_type_span(&self) -> (usize, usize) {
            self.arg_type.span
        }
        pub fn name(&self) -> Option<&ScopedIdentifier> {
            self.name.as_ref().map(|n| &n.name)
        }
    }

    impl ReturnType {
        pub fn return_type(&self) -> &Type {
            &self.return_type.value
        }
        pub fn return_type_span(&self) -> (usize, usize) {
            self.return_type.span
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

    impl Annotations {
        pub fn iter(&self) -> impl Iterator<Item = &Annotation> {
            std::iter::once(&self.first).chain(self.rest.iter())
        }
    }

    impl Docstring {
        pub fn lines(&self) -> impl Iterator<Item = &str> {
            std::iter::once(self.first.text.as_str())
                .chain(self.rest.iter().map(|line| line.text.as_str()))
        }
        pub fn joined(&self) -> String {
            self.lines().collect::<Vec<_>>().join("\n")
        }
    }

    impl Annotation {
        pub fn key(&self) -> String {
            self.key.text.clone()
        }
        /// The value rendered back to canonical text. Scalars round-trip as
        /// written; a list normalises to `[Name(a = 1, b = 2), ...]`.
        pub fn value(&self) -> String {
            match &self.value {
                // Scalars keep their plain rendering (a bare string, not a
                // quoted literal) for back-compat with existing consumers.
                AnnotationValue::Scalar(Expression::Integer(i)) => i.value.to_string(),
                AnnotationValue::Scalar(Expression::String(s)) => s.value.clone(),
                AnnotationValue::Scalar(Expression::Identifier(i)) => i.text.clone(),
                AnnotationValue::Scalar(Expression::Path(p)) => p.text.clone(),
                AnnotationValue::Scalar(Expression::FString(f)) => f.source(),
                AnnotationValue::List(list) => {
                    let calls: Vec<String> = list
                        .items
                        .iter()
                        .map(|call| {
                            let args: Vec<String> = call
                                .args
                                .iter()
                                .map(|a| {
                                    format!("{} = {}", a.name.text, a.value.as_text())
                                })
                                .collect();
                            format!("{}({})", call.name.text, args.join(", "))
                        })
                        .collect();
                    format!("[{}]", calls.join(", "))
                }
            }
        }
    }

    impl Expression {
        /// Render back to canonical source text — a quoted string literal, a
        /// bare identifier, a `::`-path, or an `f"..."`.
        pub fn as_text(&self) -> String {
            match self {
                Expression::Integer(i) => i.value.to_string(),
                Expression::String(s) => format!("{:?}", s.value),
                Expression::Identifier(i) => i.text.clone(),
                Expression::Path(p) => p.text.clone(),
                Expression::FString(f) => f.source(),
            }
        }
    }
}

// Re-export
pub use grammar::*;
