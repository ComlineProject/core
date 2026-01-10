// use crate::schema::ir::frozen::unit::FrozenUnit;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolType {
    Struct,
    Enum,
    Protocol,
    Function,
    Constant,
    Import,
}

pub struct SymbolTable<'a> {
    pub symbols: HashMap<&'a str, SymbolType>,
}

impl<'a> SymbolTable<'a> {
    pub fn new() -> Self {
        Self {
            symbols: HashMap::new(),
        }
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
}
