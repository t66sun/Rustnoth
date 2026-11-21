use std::io::{self, Write};

use crate::wml::{Node, Value};

/// Writes a parsed WML document with its original child order.
///
/// Values are quoted as plain source text, including any translatable text.
/// The output is intended for a WML parser. A native preprocessor can still
/// interpret macros in quoted values.
pub fn write_config(mut out: impl Write, node: &Node) -> io::Result<()> {
    write_node(&mut out, node, 0, false)
}

/// Writes editable WML while preserving gettext domains and translatable substrings.
/// A saved authored tutorial can then be reopened in the current UI language.
pub fn write_translatable_config(mut out: impl Write, node: &Node) -> io::Result<()> {
    write_node(&mut out, node, 0, true)
}

fn quoted(out: &mut impl Write, text: &str) -> io::Result<()> {
    write!(out, "\"{}\"", text.replace('"', "\"\""))
}

fn write_value(out: &mut impl Write, value: &Value, translatable: bool) -> io::Result<()> {
    if !translatable || value.translations.is_empty() {
        return quoted(out, &value.text);
    }
    let mut end = 0;
    for (index, segment) in value.translations.iter().enumerate() {
        if index > 0 { write!(out, " + ")?; }
        quoted(out, &value.text[end..segment.bytes.start])?;
        writeln!(out, " +")?;
        writeln!(out, "#textdomain {}", segment.domain)?;
        write!(out, "_ ")?;
        quoted(out, &value.text[segment.bytes.clone()])?;
        end = segment.bytes.end;
    }
    if end < value.text.len() {
        write!(out, " + ")?;
        quoted(out, &value.text[end..])?;
    }
    Ok(())
}

fn write_node(out: &mut impl Write, node: &Node, indent: usize, translatable: bool) -> io::Result<()> {
    for (name, value) in &node.attributes {
        write!(out, "{:indent$}{name}=", "")?;
        write_value(out, value, translatable)?;
        writeln!(out)?;
    }
    for (tag, child) in &node.children {
        writeln!(out, "{:indent$}[{tag}]", "")?;
        write_node(out, child, indent + 4, translatable)?;
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

    #[test]
    fn saved_instructions_keep_each_translated_segment_and_domain() {
        let node = parse("#textdomain tutorial\nmessage=\"{\" + _ \"Hi\"\"!\" + \"}: \" +\n#textdomain wesnoth\n_ \"Résumé\" + _ \"…\" + \".\"").unwrap();
        let mut output = Vec::new();
        write_translatable_config(&mut output, &node).unwrap();
        assert_eq!(parse(output).unwrap(), node);
    }
}
