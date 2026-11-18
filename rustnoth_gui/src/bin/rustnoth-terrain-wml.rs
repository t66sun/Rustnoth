use std::{env, error::Error, io::{self, Write}, path::PathBuf};
use engine_lite::{wml::Node, wml_emit::write_config};
use rustnoth_gui::terrain_wml::stock_rules;

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let [root] = args.as_slice() else { return Err("usage: rustnoth-terrain-wml <wesnoth-root>".into()) };
    let children = stock_rules(&PathBuf::from(root))?.into_iter()
        .map(|rule| ("terrain_graphics".to_owned(), rule)).collect();
    let mut output = io::BufWriter::new(io::stdout().lock());
    write_config(&mut output, &Node { children, ..Node::default() })?;
    output.flush()?;
    Ok(())
}
