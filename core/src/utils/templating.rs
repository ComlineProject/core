// Standard Uses

// Crate Uses

// External Uses
use eyre::Result;
use handlebars::{Handlebars, RenderError};
use serde::Serialize;


pub trait ResolvePaths<T> {
    fn resolve_paths(&self, root: &T) -> Result<Self> where Self: Sized;
}


pub fn recurse_render<T>(path: &str, entity: &T)
    -> std::result::Result<String, RenderError>
    where T: Serialize
{
    let mut reg = Handlebars::new();
    reg.set_strict_mode(true);

    let re = regex::Regex::new(r".*\{\{(.*)\}\}.*").unwrap();

    let mut res = reg.render_template(path, entity)?;
    while re.is_match(&res) {
        res = reg.render_template(&res, entity)?;
    }

    Ok(res)
}

