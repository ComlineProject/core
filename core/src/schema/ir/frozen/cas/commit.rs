// Standard Uses

// Crate Uses

// External Uses


pub enum FrozenCommit {
    Header(FrozenHeader),
    Tree(String),
    Blob(String)
}

pub enum FrozenHeader {
    AuthorId(Option<String>),
    Version(semver::Version),
}

