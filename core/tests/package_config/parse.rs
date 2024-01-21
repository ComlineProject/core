// Standard Uses
use std::ops::Range;

// Crate Uses
use crate::package_config::TEST_PACKAGE_CONFIG_PATH;

// External Uses
use comline_core::package::config;
use comline_core::package::config::idl::ast::{AssignmentUnit, ASTUnit, ListItem};
use comline_core::utils::codemap::Span;


#[test]
fn parse_package_project_new() {
    let project = config::idl::parser_new::from_path(&TEST_PACKAGE_CONFIG_PATH)
        .unwrap();

    let expected = vec![
        (Span(1), ASTUnit::Namespace(Span(2), "test".to_owned())),
        (Span(3), ASTUnit::Assignment {
            name: (Span(4), "specification_version".to_owned()),
            value: (Span(5), AssignmentUnit::Number(1))
        }),
        (Span(6), ASTUnit::Assignment {
            name: (Span(7), "schema_paths".to_owned()),
            value: (Span(8), AssignmentUnit::List(vec![
                ListItem::String(Span(9), "ping.ids".to_owned()),
                ListItem::String(Span(10), "health.ids".to_owned())

            ]))
        })
    ];

    pretty_assertions::assert_eq!(expected, project.1);

    let expected_span = vec![
        Range { start: 0, end: 26 },
        Range { start: 13, end: 26 },
        Range { start: 26, end: 52 },
        Range { start: 27, end: 48 },
        Range { start: 51, end: 52 },
        Range { start: 52, end: 88 },
        Range { start: 54, end: 66 },
        Range { start: 69, end: 88 },
        Range { start: 76, end: 84 },
    ];

    // let contents = config.0.files().first().unwrap().contents();
    let mut i = 0;
    while i < project.0.files().first().unwrap().items().borrow().values().len() {
        i += 1;

        /*
        let span = &expected_span[i - 1];
        println!("next is {:?}", config.0.files()[0].range_of(Span(i+1)));
        println!("{:?} - {:?}", span, contents.get(span.clone()).unwrap());
        */

        assert_eq!(
            Some(&expected_span[i - 1]),
            project.0.files()[0].range_of(Span(i)).as_ref()
        );
    };
}

