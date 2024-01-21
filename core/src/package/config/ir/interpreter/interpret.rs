// Standard Uses

// Crate Uses
use crate::package::config::ir::context::ProjectContext;
use crate::package::config::ir::frozen::FrozenUnit;
use crate::package::config::ir::interpreter::freezing;

// External Uses


#[allow(unused)]
pub fn interpret_context(mut context: &ProjectContext)
    -> Result<Vec<FrozenUnit>, Box<dyn snafu::Error>>
{
    let mut interpreted = vec![];

    for node in &context.config.1 {
        let file = context.config.0.files().first().unwrap();
        // let span = file.range_of(node.0).unwrap();

        interpreted.append(
            &mut freezing::interpret_node_into_frozen(context, &node.1)?
        );
    }

    Ok(interpreted)
    // freezing::into_frozen_whole(&context, interpreted)
}
