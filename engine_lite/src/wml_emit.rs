use std::io::{self, Write};

use crate::wml::Node;

/// Writes a parsed WML document with its original child order.
///
/// Values are quoted as plain source text, including any translatable text.
/// The output is intended for a WML parser. A native preprocessor can still
/// interpret macros in quoted values.
pub fn write_config(mut out: impl Write, node: &Node) -> io::Result<()> {
    write_node(&mut out, node, 0)
}

fn write_node(out: &mut impl Write, node: &Node, indent: usize) -> io::Result<()> {
    for (name, value) in &node.attributes {
        let quoted_text = value.text.replace('"', "\"\"");
        writeln!(out, "{:indent$}{name}=\"{quoted_text}\"", "")?;
    }
    for (tag, child) in &node.children {
        writeln!(out, "{:indent$}[{tag}]", "")?;
        write_node(out, child, indent + 4)?;
        writeln!(out, "{:indent$}[/{tag}]", "")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wml::parse;

    #[test]
    fn preserves_quoted_text_and_ordered_children() {
        let source = r#"z=""
a=<<say "hello" + {name}
next line>>
[unit]
    id=first
[/unit]
[unit]
    id=second
    [attack]
        id=sword
    [/attack]
[/unit]
"#;
        let node = parse(source).unwrap();
        let mut output = Vec::new();
        write_config(&mut output, &node).unwrap();

        assert_eq!(
            output.as_slice(),
            br#"a="say ""hello"" + {name}
next line"
z=""
[unit]
    id="first"
[/unit]
[unit]
    id="second"
    [attack]
        id="sword"
    [/attack]
[/unit]
"#
        );
        assert_eq!(parse(output).unwrap(), node);
    }

    #[test]
    fn writes_translatable_values_as_plain_source_text() {
        let node = parse(r#"name=_ "Wesnoth""#).unwrap();
        let mut output = Vec::new();
        write_config(&mut output, &node).unwrap();

        assert_eq!(output, b"name=\"Wesnoth\"\n");
    }
}
