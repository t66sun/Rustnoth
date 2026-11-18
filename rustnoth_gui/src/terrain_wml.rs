//! Expands the terrain-graphics macros shipped in the Wesnoth data directory.
use std::{
    collections::{BTreeMap, HashMap},
    fs, io,
    path::Path,
};

use engine_lite::wml::{self, Node};

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

type Parameters = BTreeMap<String, String>;

#[derive(Default)]
struct Definition {
    positional: Vec<String>,
    defaults: Parameters,
    body: String,
}

#[derive(Default)]
struct TerrainSource {
    macros: HashMap<String, Definition>,
    output: String,
}

/// Reads the stock non-editor terrain rule set, preserving native definition and rule order.
pub fn stock_rules(root: &Path) -> io::Result<Vec<Node>> {
    let core = root.join("data/core");
    let mut definitions = Vec::new();
    for entry in fs::read_dir(core.join("terrain-graphics"))? {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == "cfg") {
            definitions.push(path);
        }
    }
    definitions.sort();
    definitions.push(core.join("terrain-graphics.cfg"));

    let mut source = TerrainSource::default();
    for path in definitions {
        source.read(&fs::read_to_string(&path)?)
            .map_err(|e| invalid(format!("{}: {e}", path.display())))?;
    }
    let root = wml::parse(source.output).map_err(|e| invalid(e.to_string()))?;
    Ok(root.children.into_iter().filter_map(|(tag, node)| (tag == "terrain_graphics").then_some(node)).collect())
}

impl TerrainSource {
    fn flush(&mut self, pending: &mut String) -> io::Result<()> {
        let expanded = self.expand(pending, &Parameters::new(), 0)?;
        self.output.push_str(&expanded);
        pending.clear();
        Ok(())
    }

    fn read(&mut self, file: &str) -> io::Result<()> {
        let mut definition: Option<(String, Definition)> = None;
        let mut default: Option<(String, String)> = None;
        let mut pending = String::new();
        let mut active = vec![true];
        for line in file.lines() {
            let (prefix, directive) = line.split_once('#').unwrap_or((line, ""));
            let mut words = directive.split_ascii_whitespace();
            let command = words.next();
            if command == Some("ifdef") {
                // The stock terrain sources use this to exclude editor-only overlays.
                let enabled = words.next().is_some_and(|name| self.macros.contains_key(name));
                active.push(active.last() == Some(&true) && enabled);
                continue;
            }
            if command == Some("endif") {
                if active.len() == 1 { return Err(invalid("unmatched #endif")); }
                active.pop();
                continue;
            }
            if active.last() != Some(&true) { continue; }

            let text = if let Some((_, value)) = &mut default { value }
                else if let Some((_, def)) = &mut definition { &mut def.body }
                else { &mut pending };
            text.push_str(prefix);
            // Inline terminators are used to splice filenames and animation frame expressions.
            if !matches!(command, Some("endarg" | "enddef")) { text.push('\n'); }
            match command {
                Some("define") => {
                    self.flush(&mut pending)?;
                    let name = words.next().ok_or_else(|| invalid("missing macro name"))?.to_owned();
                    definition = Some((name, Definition {
                        positional: words.map(str::to_owned).collect(), ..Definition::default()
                    }));
                }
                Some("arg") => {
                    let name = words.next().ok_or_else(|| invalid("missing optional argument name"))?;
                    default = Some((name.to_owned(), String::new()));
                }
                Some("endarg") => {
                    let (name, value) = default.take().ok_or_else(|| invalid("unmatched #endarg"))?;
                    let (_, def) = definition.as_mut().ok_or_else(|| invalid("#arg outside #define"))?;
                    def.defaults.insert(name, value);
                }
                Some("enddef") => {
                    let (name, def) = definition.take().ok_or_else(|| invalid("unmatched #enddef"))?;
                    self.macros.insert(name, def);
                }
                Some("undef") => {
                    self.flush(&mut pending)?;
                    if let Some(name) = words.next() { self.macros.remove(name); }
                }
                _ => {}
            }
        }
        if definition.is_some() || default.is_some() || active.len() != 1 {
            return Err(invalid("unterminated terrain macro directive"));
        }
        self.flush(&mut pending)
    }

    fn expand(&self, mut source: &str, params: &Parameters, depth: usize) -> io::Result<String> {
        if depth > 100 { return Err(invalid("terrain macro nesting exceeds 100 calls")); }
        let mut expanded = String::with_capacity(source.len());
        while let Some((literal, rest)) = source.split_once('{') {
            expanded.push_str(literal);
            source = rest;
            expanded.push_str(&self.call(&mut source, params, depth + 1)?);
        }
        expanded.push_str(source);
        Ok(expanded)
    }

    fn call(&self, input: &mut &str, caller: &Parameters, depth: usize) -> io::Result<String> {
        if depth > 100 { return Err(invalid("terrain macro nesting exceeds 100 calls")); }
        let mut args = Vec::new();
        let mut chunk = String::new();
        let mut started = false;
        let mut quoted = false;
        let mut grouped = false;
        loop {
            let c = input.chars().next().ok_or_else(|| invalid("unterminated terrain macro call"))?;
            *input = &input[c.len_utf8()..];
            match c {
                '{' => {
                    chunk.push_str(&self.call(input, caller, depth + 1)?);
                    started = true;
                }
                '"' => {
                    quoted = !quoted;
                    chunk.push(c);
                    started = true;
                }
                ')' if grouped && !quoted => {
                    args.push(std::mem::take(&mut chunk));
                    started = false;
                    grouped = false;
                }
                '(' if !quoted && !grouped => {
                    if started { args.push(std::mem::take(&mut chunk)); }
                    started = true;
                    grouped = true;
                }
                '}' if !quoted && !grouped => {
                    if started { args.push(chunk); }
                    break;
                }
                c if c.is_ascii_whitespace() && !grouped && !quoted => {
                    if started { args.push(std::mem::take(&mut chunk)); }
                    started = false;
                }
                c => {
                    chunk.push(c);
                    started = true;
                }
            }
        }
        let (symbol, args) = args.split_first().ok_or_else(|| invalid("empty terrain macro call"))?;
        let symbol = symbol.trim();
        if let Some(value) = caller.get(symbol) {
            if !args.is_empty() { return Err(invalid(format!("parameter {symbol} used as macro"))); }
            return Ok(value.clone());
        }
        let def = self.macros.get(symbol).ok_or_else(|| invalid(format!("unknown terrain macro {symbol}")))?;
        if args.len() < def.positional.len() {
            return Err(invalid(format!("{symbol} expects {} positional arguments, got {}", def.positional.len(), args.len())));
        }
        let mut bound: Parameters = def.positional.iter().cloned().zip(args.iter().cloned()).collect();
        for arg in &args[def.positional.len()..] {
            let (key, value) = arg.split_once('=')
                .ok_or_else(|| invalid(format!("extra argument {arg:?} in {symbol}")))?;
            // The native preprocessor ignores overrides of unknown option names.
            if def.defaults.contains_key(key.trim()) { bound.insert(key.trim().to_owned(), value.to_owned()); }
        }
        for (key, default) in &def.defaults {
            if !bound.contains_key(key) {
                let value = self.expand(default, &bound, depth)?;
                bound.insert(key.clone(), value);
            }
        }
        self.expand(&def.body, &bound, depth)
            .map_err(|e| invalid(format!("{symbol}: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_nested_builders_optional_defaults_and_grouped_terrain_filters_in_order() {
        let mut source = TerrainSource::default();
        source.read(r#"#define SPRITE STEM
{STEM}.png#enddef
#define RULE BUILDER TERRAIN STEM
#arg FLAG
base#endarg
[terrain_graphics]
[tile]
x,y=0,1
type={TERRAIN}
set_no_flag={FLAG}
[image]
name={{BUILDER} {STEM}}
[/image]
[/tile]
[/terrain_graphics]
#enddef
#define TWO STEM
{RULE SPRITE (!,G*,W*) {STEM}}
{RULE SPRITE (*) {STEM} (FLAG="forest edge")}
[+terrain_graphics]
probability=17
[/terrain_graphics]
#enddef
#ifdef EDITOR
{UNAVAILABLE}
#endif
{TWO forest/pine}
"#).unwrap();
        let expanded = wml::parse(source.output).unwrap();
        assert_eq!(expanded.children.len(), 2);
        let first = &expanded.children[0].1.children[0].1;
        assert_eq!(first.attribute("type"), Some("!,G*,W*"));
        assert_eq!(first.attribute("set_no_flag"), Some("base"));
        assert_eq!(first.attribute("y"), Some("1"));
        assert_eq!(first.children[0].1.attribute("name"), Some("forest/pine.png"));
        let second = &expanded.children[1].1;
        assert_eq!(second.attribute("probability"), Some("17"));
        assert_eq!(second.children[0].1.attribute("set_no_flag"), Some("forest edge"));
    }
}
