// Standard Uses
use std::path::Path;
use std::sync::Arc;

// Crate Uses
use crate::package::config::idl::ast::{
    AssignmentUnit, ASTUnit, DictKeyValue,
    ListItem, SourcedWhole, SpannedUnit
};
use crate::utils::codemap::{CodeMap, FileMap};

// External Uses
use eyre::{bail, Result};
use pest::{iterators::Pair, Parser};
use pest_derive::Parser;


#[derive(Parser)]
#[grammar = "package/config/idl/idc.pest"]
pub struct ProjectParser;

#[allow(unused)]
pub fn from_path(path: &Path) -> Result<SourcedWhole> {
    if !path.exists() { bail!("Path doesn't exist: {:?}", path) }

    let source = std::fs::read_to_string(path).unwrap();

    let sourced_whole = parse_source(
        source.clone(),
        path.file_name().unwrap().to_str().unwrap().to_owned()
    );

    sourced_whole
}


pub fn parse_source(source: String, name: String) -> Result<SourcedWhole> {
    let mut codemap = CodeMap::new();
    let file = codemap.insert_file(name, source.clone());

    let pairs = ProjectParser::parse(Rule::syntax, source.as_str())?;
    let mut units = vec![];

    for pair in pairs {
        if let Ok(u) = parse_inner(pair, &file) {
            units.push(u)
        }
    }

    Ok((codemap, units))
}

pub fn parse_inner(pair: Pair<'_, Rule>, file: &Arc<FileMap>) -> Result<SpannedUnit> {
    match pair.as_rule() {
        Rule::congregation => {
            let span = pair.as_span();
            let namespace_pair = pair.into_inner().next().unwrap();
            let namespace_span = namespace_pair.as_span();
            let namespace = namespace_pair.as_str().to_owned();

            Ok((
                file.insert_span(span.start(), span.end()),
                ASTUnit::Namespace(
                    file.insert_span(namespace_span.start(), namespace_span.end()),
                    namespace
                )
            ))
        },
        Rule::assignment => {
            let span = pair.as_span();
            let mut inner = pair.into_inner();

            let name_pair = inner.next().unwrap();
            let name_span = name_pair.as_span();

            let value_pair = inner.next().unwrap();
            let value_span = value_pair.as_span();

            Ok((
                file.insert_span(span.start(), span.end()),
                ASTUnit::Assignment {
                    name: (
                        file.insert_span(name_span.start(), name_span.end()),
                        name_pair.as_str().to_owned()
                    ),
                    value: (
                        file.insert_span(value_span.start(), value_span.end()),
                        parse_assignment(value_pair, file)?
                    )
                }
            ))
        }
        missing => panic!("Rule not implemented {:?}", missing)
        // _ => { bail!("")}
    }
}


#[allow(unused)]
fn parse_assignment(pair: Pair<'_, Rule>, file: &Arc<FileMap>) -> Result<AssignmentUnit> {
    let unit = match pair.as_rule() {
        Rule::number => {
            Ok(AssignmentUnit::Number(pair.as_str().parse().unwrap()))
        },
        Rule::string => {
            Ok(AssignmentUnit::String(pair.as_str().to_owned()))
        },
        Rule::dictionary => {
            let span = pair.as_span();
            let span = file.insert_span(span.start(), span.end());

            let mut key_values = vec![];

            for item in pair.into_inner() {
                let mut inner = item.into_inner();

                let key_pair = inner.next().unwrap();
                let key_span = key_pair.as_span();
                let key_span = file.insert_span(key_span.start(), key_span.end());
                let key = key_pair.as_str().to_owned();

                let value_pair = inner.next().unwrap();
                let value_span = value_pair.as_span();
                let value_span = file.insert_span(value_span.start(), value_span.end());
                let value = parse_assignment(value_pair, file)?;

                key_values.push(DictKeyValue {
                    key: (key_span, key),
                    value: (value_span, value)
                });
            }

            Ok(AssignmentUnit::Dictionary(key_values))
        }
        Rule::list => {
            let span = pair.as_span();
            let span = file.insert_span(span.start(), span.end());

            let mut items = vec![];

            for item in pair.into_inner() {
                items.push(parse_list_item(item, file)?);
            }

            Ok(AssignmentUnit::List(items))
        },
        Rule::domain_namespaced => {
            Ok(AssignmentUnit::String(pair.as_str().to_owned()))
        },
        Rule::string_inner => {
            Ok(AssignmentUnit::String(pair.as_str().to_owned()))
        },
        missing => panic!("Rule not implemented: {:?}", missing)
    };

    unit
}


fn parse_list_item(pair: Pair<'_, Rule>, file: &Arc<FileMap>) -> Result<ListItem> {
    let span = pair.as_span();
    let item_span = file.insert_span(span.start(), span.end());

    match pair.as_rule() {
        Rule::number => {
            Ok(ListItem::Number(item_span, pair.as_str().parse().unwrap()))
        },
        Rule::string_inner => {
            Ok(ListItem::String(item_span, pair.as_str().to_owned()))
        },
        Rule::path => {
            Ok(ListItem::Path(item_span, pair.as_str().to_owned()))
        },
        Rule::domain_namespaced => {
            Ok(ListItem::String(item_span, pair.as_str().to_owned()))
        },
        missing => panic!("Rule not implemented: {:?}", missing)
    }
}

