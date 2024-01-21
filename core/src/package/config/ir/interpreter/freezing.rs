// Standard Uses

// Crate Uses
use crate::package::config::idl::ast::{AssignmentUnit, ASTUnit, DictKeyValue, ListItem};
use crate::package::config::ir::context::ProjectContext;
use crate::package::config::ir::frozen::{
    FrozenUnit, FrozenWhole, LanguageDetails, PublishRegistry, RegistryKind
};
use crate::utils::codemap::Span;

// External Uses


#[allow(unused)]
pub fn interpret_node_into_frozen(
    context: &ProjectContext, node: &ASTUnit
) -> Result<Vec<FrozenUnit>, Box<dyn snafu::Error>>
{
    use crate::package::config::idl::ast::ASTUnit::*;
    match node {
        Namespace(span, name) => {
            Ok(vec![FrozenUnit::Namespace(name.clone())])
        },
        Assignment {name, value} => {
            interpret_assignment(context, name,value)
        },
        missing => unimplemented!("AST Node not implemented '{:?}'", missing)
    }
}

pub fn interpret_assignment(
    context: &ProjectContext, name: &(Span, String), node: &(Span, AssignmentUnit)
) -> Result<Vec<FrozenUnit>, Box<dyn snafu::Error>> {
    let result = match &*name.1 {
        "specification_version" => {
            let AssignmentUnit::Number(version) = &node.1 else {
                panic!(
                    "'specification_version' should be a number(up to unsigned long integer, \
                    got something else instead."
                )
            };

            vec![FrozenUnit::SpecificationVersion(*version as u8)]
        },
        /*
        "schemas_source_path" => {
            todo!()
        }
        */
        /*
        "schema_paths" => {
            let AssignmentUnit::List(paths) = &node.1 else {
                panic!("'schema_paths' should be a list of paths, got something else instead.")
            };

            let mut solved = vec![];
            for path in paths {
                let ListItem::String(.., path) = path else {
                    panic!("Expected path, got something else instead")
                };

                let schema_file = context.find_schema_by_filename(path);

                if schema_file.is_none() { panic!("No schema found with the path: '{}'", path) }

                solved.push(FrozenUnit::SchemaPath(path.clone()));
            }

            solved
        },
        */
        "code_generation" => {
            let AssignmentUnit::Dictionary(items) = &node.1 else {
                panic!("Expected dictionary, got something else instead")
            };

            interpret_assignment_code_generation(items)?
        },
        "publish_registries" => {
            let AssignmentUnit::Dictionary(items) = &node.1 else {
                panic!("Expected dictionary, got something else instead")
            };

            interpret_assigment_publish_registries(items)?
        },
        any => {
            panic!("Assignment '{}' is not a valid assignment", any)
        }
    };

    Ok(result)
}

fn interpret_assignment_code_generation(items: &Vec<DictKeyValue>)
    -> Result<Vec<FrozenUnit>, Box<dyn snafu::Error>>
{
    let mut languages = vec![];

    use AssignmentUnit::*;
    for kv in items {
        let key = &kv.key;
        let Dictionary(value) = &kv.value.1 else {
            panic!("Not expected")
        };

        match &*key.1 {
            "languages" => {
                for lang_details in value {
                    let name = &lang_details.key;
                    let Dictionary(details) = &lang_details.value.1 else {
                        panic!("Not expected here")
                    };

                    let mut versions = vec![];
                    let path = None;

                    for assignment in details {
                        match &*assignment.key.1 {
                            "package_versions" => {
                                let List(items) = &assignment.value.1 else {
                                    panic!("Wrong kind")
                                };

                                for item in items {
                                    let ListItem::String(_, version) = item else {
                                        panic!("Not kind")
                                    };

                                    versions.push(version.clone())

                                }
                            },
                            other => { panic!("Not expected another: {:?}", other) }
                        }
                    }

                    languages.push(
                        FrozenUnit::CodeGeneration(
                            LanguageDetails {
                                name: name.1.clone(), versions,
                                generation_path: path,
                            }
                        )
                    );
                }
            },
            other => panic!("Key not allowed here: {}", other)
        }
    }

    Ok(languages)
}

fn interpret_assigment_publish_registries(
    items: &Vec<DictKeyValue>
) -> Result<Vec<FrozenUnit>, Box<dyn snafu::Error>> {
    let mut targets = vec![];

    use AssignmentUnit::*;
    for kv in items {
        let key = &kv.key;

        let target = match &kv.value.1 {
            String(name) => {
                // TODO: We might only need reference to variables,
                //       a string wouldn't be much useful
                FrozenUnit::PublishRegistry((name.clone(), PublishRegistry {
                    kind: RegistryKind::LocalStorage,
                    uri: "none".to_string(),
                }))
            }
            Reference(_reference) => {
                panic!()
            }
            Dictionary(items) => {
                let mut url = None;
                let mut registry_kind = None;

                for item in items {
                    match &*item.key.1 {
                        "uri" => {
                            if let String(s) = &item.value.1 {
                                // TODO: Needs to parse URI to decide the kind
                                registry_kind = Some(RegistryKind::LocalStorage);
                                url = Some(s);
                            } else {
                                panic!(
                                    "URI should be a string with the format:\n\
                                    - (local|server)+(http|https|ssh)://(path)"
                                )
                            };
                        },
                        /*
                        "method" => {
                            if let String(s) = &item.value.1 {
                                method = Some(s);
                            } else { panic!("Needs a proper method") };
                        },
                        */
                        other => panic!("Key not allowed here: {}", other)
                    }
                }

                FrozenUnit::PublishRegistry((key.1.clone(), PublishRegistry {
                    kind: registry_kind.unwrap(),
                    uri: url.unwrap().clone(),
                }))
            },
            other => panic!(
                "Can only be a reference or a dict, got {:?} instead", other
            )
        };

        targets.push(target);
    }

    Ok(targets)
}

#[allow(unused)]
pub fn into_frozen_whole(
    context: &ProjectContext, interpreted: Vec<FrozenUnit>
) -> Result<FrozenWhole, Box<dyn snafu::Error>>
{
    todo!()
    // Ok((Rc::from(context), interpreted))
}

