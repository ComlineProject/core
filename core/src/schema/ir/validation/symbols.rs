// use crate::schema::ir::frozen::unit::FrozenUnit;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolType {
    Struct,
    Enum,
    Protocol,
    Function,
    Constant,
    Import,
    Validator,
    TypeAlias,
    Error,
}

impl SymbolType {
    /// Whether a name of this kind may stand in a type position - a field's
    /// type, a function's argument or return, a type alias' target.
    ///
    /// Presence in the table used to be the whole test, so every kind here
    /// was usable as a type: `field: SomeProtocol`, `field: SOME_CONST` and
    /// `field: SomeValidator` all type-checked. An `Import` is opaque (the
    /// imported schema is not necessarily parsed here), so it stays
    /// permitted.
    pub const fn usable_as_type(self) -> bool {
        match self {
            SymbolType::Struct
            | SymbolType::Enum
            | SymbolType::TypeAlias
            | SymbolType::Import => true,
            SymbolType::Protocol
            | SymbolType::Function
            | SymbolType::Constant
            | SymbolType::Validator
            | SymbolType::Error => false,
        }
    }

    /// How to name this kind in a diagnostic, with its article - "an error",
    /// "a protocol".
    pub const fn describe(self) -> &'static str {
        match self {
            SymbolType::Struct => "a struct",
            SymbolType::Enum => "an enum",
            SymbolType::Protocol => "a protocol",
            SymbolType::Function => "a function",
            SymbolType::Constant => "a constant",
            SymbolType::Import => "an import",
            SymbolType::Validator => "a validator",
            SymbolType::TypeAlias => "a type alias",
            SymbolType::Error => "an error",
        }
    }
}

pub struct SymbolTable<'a> {
    pub symbols: HashMap<&'a str, SymbolType>,
    /// Bare names a `use` brought into scope: the alias in `use ns::Name as X`,
    /// otherwise the trailing segment of `use ns::Name`. Kept apart from
    /// `symbols` so it never causes a "duplicate definition" and a real local
    /// declaration always shadows it.
    bare_imports: HashSet<&'a str>,
}

impl<'a> SymbolTable<'a> {
    pub fn new() -> Self {
        Self {
            symbols: HashMap::new(),
            bare_imports: HashSet::new(),
        }
    }

    /// Record a bare name made available by a `use`.
    pub fn add_bare_import(&mut self, name: &'a str) {
        self.bare_imports.insert(name);
    }

    pub fn insert(&mut self, name: &'a str, kind: SymbolType) -> Result<(), SymbolType> {
        if let Some(existing) = self.symbols.get(name) {
            return Err(*existing);
        }
        self.symbols.insert(name, kind);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<SymbolType> {
        self.symbols.get(name).cloned()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.symbols.contains_key(name)
    }

    /// Whether `name` is a bare reference a `use` brought into scope, Rust-style
    /// (an alias, or the trailing segment of a plain `use ns::Name`). The
    /// qualified `ns::Name` form resolves through `contains` instead.
    pub fn is_imported_bare(&self, name: &str) -> bool {
        self.bare_imports.contains(name)
    }
}
