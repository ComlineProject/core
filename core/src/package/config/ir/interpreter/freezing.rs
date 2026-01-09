// Standard Uses

// Crate Uses
use crate::package::config::idl::grammar::{
    Assignment, Key, Value,
    self
};
use crate::package::config::ir::context::ProjectContext;
use crate::package::config::ir::frozen::{
    FrozenUnit, FrozenWhole, LanguageDetails, PublishRegistry, RegistryKind
};
// use crate::utils::codemap::Span;

// External Uses


#[allow(unused)]
pub fn interpret_node_into_frozen(
    context: &ProjectContext, node: &Assignment
) -> Result<Vec<FrozenUnit>, Box<dyn snafu::Error>>
{
   interpret_assignment(context, node)
}

pub fn interpret_assignment(
    context: &ProjectContext, node: &Assignment
) -> Result<Vec<FrozenUnit>, Box<dyn snafu::Error>> {
    let key_str = match &node.key {
        Key::Identifier(id) => id.value.clone(),
        Key::Namespaced(ns) => ns.value.clone(),
        Key::VersionMeta(vm) => vm.value.clone(),
        Key::DependencyAddress(da) => da.value.clone(),
    };

    let result = match key_str.as_str() {
        "specification_version" => {
            let Value::Number(version) = &node.value else {
                panic!(
                    "'specification_version' should be a number, \
                    got something else instead."
                )
            };
            
            // Should parse integer
            let version_num: u8 = version.value.parse().expect("Invalid version number");
            vec![FrozenUnit::SpecificationVersion(version_num)]
        },
        /*
        "schemas_source_path" => {
            todo!()
        }
        */
        /*
        "schema_paths" => {
            let Value::List(paths) = &node.value else {
                panic!("'schema_paths' should be a list of paths")
            };

            let mut solved = vec![];
            for path_val in &paths.items {
                let Value::String(path) = path_val else {
                    panic!("Expected path string")
                };

                let schema_file = context.find_schema_by_filename(&path.value);

                if schema_file.is_none() { panic!("No schema found with the path: '{}'", path.value) }

                solved.push(FrozenUnit::SchemaPath(path.value.clone()));
            }

            solved
        },
        */
        "code_generation" => {
            let Value::Dictionary(items) = &node.value else {
                panic!("Expected dictionary for code_generation")
            };

            interpret_assignment_code_generation(items.assignments.as_ref())?
        },
        "publish_registries" => {
            let Value::Dictionary(items) = &node.value else {
                panic!("Expected dictionary for publish_registries")
            };

            interpret_assigment_publish_registries(items.assignments.as_ref())?
        },
        any => {
            // panic!("Assignment '{}' is not a valid assignment", any)
            // Allow unknown assignments for now or warn?
            // panic for now to match behavior
            panic!("Assignment '{}' is not a valid assignment", any)
        }
    };

    Ok(result)
}

fn interpret_assignment_code_generation(items: &Vec<Assignment>)
    -> Result<Vec<FrozenUnit>, Box<dyn snafu::Error>>
{
    let mut languages = vec![];

    for assignment in items {
        let key_str = match &assignment.key {
             Key::Identifier(id) => id.value.clone(),
             Key::Namespaced(ns) => ns.value.clone(),
             Key::VersionMeta(vm) => vm.value.clone(),
             Key::DependencyAddress(da) => da.value.clone(),
        };
        
        match key_str.as_str() {
            "languages" => {
                 // Value should be Dictionary of Language -> Details
                 let Value::Dictionary(lang_dict) = &assignment.value else {
                     panic!("languages must be a dictionary") 
                 };
                 
                 for lang_assign in &lang_dict.assignments {
                     let lang_name = match &lang_assign.key {
                         Key::Identifier(id) => id.value.clone(),
                         Key::Namespaced(ns) => ns.value.clone(),
                         Key::VersionMeta(vm) => vm.value.clone(),
                         Key::DependencyAddress(da) => da.value.clone(),
                     };
                     
                     let Value::Dictionary(details) = &lang_assign.value else {
                         panic!("Language details must be a dictionary")
                     };
                     
                     let mut versions = vec![];
                     let path = None;
                     
                     for detail in &details.assignments {
                         let detail_key = match &detail.key {
                             Key::Identifier(id) => id.value.clone(),
                             Key::Namespaced(ns) => ns.value.clone(),
                             Key::VersionMeta(vm) => vm.value.clone(),
                             Key::DependencyAddress(da) => da.value.clone(),
                         };
                         
                         match detail_key.as_str() {
                             "package_versions" => {
                                 let Value::List(v_list) = &detail.value else {
                                     panic!("package_versions must be a list")
                                 };
                                 
                                 for item in &v_list.items {
                                     let val_str = match item {
                                         Value::String(s) => s.value.clone(),
                                         Value::Identifier(id) => id.value.clone(),
                                         _ => panic!("Version must be a string or identifier")
                                     };
                                     versions.push(val_str);
                                 }
                             },
                             other => panic!("Not expected: {}", other),
                         }
                     }
                     
                     languages.push(
                        FrozenUnit::CodeGeneration(
                            LanguageDetails {
                                name: lang_name, versions,
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
    items: &Vec<Assignment>
) -> Result<Vec<FrozenUnit>, Box<dyn snafu::Error>> {
    let mut targets = vec![];

    for assignment in items {
        let key_str = match &assignment.key {
             Key::Identifier(id) => id.value.clone(),
             Key::Namespaced(ns) => ns.value.clone(),
             Key::VersionMeta(vm) => vm.value.clone(),
             Key::DependencyAddress(da) => da.value.clone(),
        };
        
        let target = match &assignment.value {
            Value::String(name) => {
                FrozenUnit::PublishRegistry((key_str, PublishRegistry {
                    kind: RegistryKind::LocalStorage,
                    uri: "none".to_string(),
                }))
            }
            Value::Identifier(name) => {
                 FrozenUnit::PublishRegistry((key_str, PublishRegistry {
                    kind: RegistryKind::LocalStorage, // TODO: logic for identifier registry?
                    uri: "none".to_string(),
                }))
            }
            Value::Namespaced(ns) => {
                 FrozenUnit::PublishRegistry((key_str, PublishRegistry {
                    kind: RegistryKind::LocalStorage, // TODO: resolve namespaced registry
                    uri: "none".to_string(),
                }))
            }
            Value::Dictionary(dict) => {
                let mut url = None;
                let mut registry_kind = None;
                
                for item in &dict.assignments {
                    let item_key = match &item.key {
                         Key::Identifier(id) => id.value.clone(),
                         Key::Namespaced(ns) => ns.value.clone(),
                         Key::VersionMeta(vm) => vm.value.clone(),
                         Key::DependencyAddress(da) => da.value.clone(),
                     };
                     
                     match item_key.as_str() {
                         "uri" => {
                             if let Value::String(s) = &item.value {
                                 registry_kind = Some(RegistryKind::LocalStorage);
                                 url = Some(s.value.clone());
                             } else {
                                 panic!("URI should be a string")
                             }
                         },
                         // method...
                         other => panic!("Key not allowed here: {}", other)
                     }
                }
                
                FrozenUnit::PublishRegistry((key_str, PublishRegistry {
                    kind: registry_kind.unwrap(),
                    uri: url.unwrap(),
                }))
            },
            other => panic!("Invalid registry value: {:?}", other)
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
}
