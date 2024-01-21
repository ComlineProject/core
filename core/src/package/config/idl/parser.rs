// Standard Uses

// Local Uses

// External Uses




// TODO: This whole module is old and should be deleted, the module `parser_new` is the correct
//       one, and should be renamed to just `parser`

/*
pub fn from_pat\h(path: &Path) -> Result<super::ast::WholeUnit> {
    if !path.exists() { bail!("Path doesn't exist: {:?}", path) }

    from_path_str(path.to_str().unwrap())
}

pub fn from_path_str(path: &str) -> Result<super::ast::WholeUnit> {
    let raw = std::fs::read_to_string(path).unwrap();
    let vunit = parse_into_unit(raw.clone().as_str()).unwrap();
    let vindex = VIndex {
        meta: UnitIndex::Index { path: path.to_string(), source: raw },
        nodes: vec![]
    };

    Ok((vindex, vunit))
}

pub fn parse_into_unit(content: &str) -> Result<super::ast::VUnit> {
    let pairs = IDLParser::parse(Rule::syntax, content)?;
    let mut unit = vec![];

    for pair in pairs { unit.push(parse_inner(pair).unwrap()) }

    Ok(unit)
}

#[allow(unused)]
pub fn parse_inner(pair: Pair<Rule>) -> Result<super::ast::ASTUnit> {
    match pair.as_rule() {
        missing => { panic!("Rule not implemented: {:?}", missing) }
    }
}

*/
