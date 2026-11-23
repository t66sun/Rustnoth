use engine_lite::wml::{self, Node};
use std::{
    collections::{HashMap, HashSet},
    error::Error,
};

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Span {
    Text(String),
    Emphasis(String),
    Heading(String),
    Link(String, String),
    Image(String),
}

fn property<'a>(mut record: &'a str, key: &str) -> Option<&'a str> {
    while let Some((name, rest)) = record.trim_start().split_once('=') {
        let rest = rest.trim_start();
        let (value, tail) = if let Some(quote @ ('\'' | '"')) = rest.chars().next() {
            rest[1..].split_once(quote)?
        } else {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            (&rest[..end], &rest[end..])
        };
        if name.trim() == key {
            return Some(value);
        }
        record = tail;
    }
    None
}

fn markup(mut input: &str) -> Vec<Span> {
    let mut runs = Vec::new();
    while let Some(at) = input.find('<') {
        if at > 0 {
            runs.push(Span::Text(input[..at].to_owned()));
        }
        input = &input[at..];
        let Some((name, rest)) = input[1..].split_once('>') else {
            break;
        };
        if !matches!(
            name,
            "ref" | "bold" | "italic" | "header" | "img" | "format" | "jump"
        ) {
            runs.push(Span::Text(format!("<{name}>")));
            input = rest;
            continue;
        }
        let closing = format!("</{name}>");
        let Some((args, tail)) = rest.split_once(&closing) else {
            runs.push(Span::Text(format!("<{name}>")));
            input = rest;
            continue;
        };
        let text = property(args, "text").unwrap_or("").to_owned();
        match name {
            "ref" => runs.push(Span::Link(
                text,
                property(args, "dst").unwrap_or("").to_owned(),
            )),
            "img" => {
                if let Some(src) = property(args, "src") {
                    runs.push(Span::Image(src.to_owned()));
                }
            }
            "header" => runs.push(Span::Heading(text)),
            "italic" | "bold" => runs.push(Span::Emphasis(text)),
            "jump" => runs.push(Span::Text("  ".to_owned())),
            _ => runs.push(Span::Text(text)),
        }
        input = tail;
    }
    if !input.is_empty() {
        runs.push(Span::Text(input.to_owned()));
    }
    runs
}

#[derive(Debug)]
pub(super) struct Topic {
    pub title: String,
    pub spans: Vec<Span>,
}

#[derive(Default)]
struct Section {
    title: String,
    sections: Vec<String>,
    topics: Vec<String>,
}

#[derive(Default)]
pub(super) struct Book {
    root: Section,
    sections: HashMap<String, Section>,
    pub topics: HashMap<String, Topic>,
}

pub(super) struct Row<'a> {
    pub depth: usize,
    pub id: &'a str,
    pub title: &'a str,
    pub folder: bool,
}

fn section(node: &Node) -> Section {
    let ids = |key| {
        node.attribute(key)
            .unwrap_or("")
            .split(',')
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
            .collect()
    };
    Section {
        title: node.attribute("title").unwrap_or("").into(),
        sections: ids("sections"),
        topics: ids("topics"),
    }
}

impl Book {
    pub fn insert_unit(&mut self, id: String, topic: Topic) {
        let section = self.sections.get_mut("units").unwrap_or(&mut self.root);
        if !section.topics.contains(&id) { section.topics.push(id.clone()); }
        self.topics.insert(id, topic);
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, Box<dyn Error>> {
        let book = Self::read(bytes)?;
        if book.initial().is_none() {
            return Err("The help contents contain no readable starting topic".into());
        }
        Ok(book)
    }

    pub fn read(bytes: &[u8]) -> Result<Self, Box<dyn Error>> {
        let root = wml::parse(bytes)?;
        let mut book = Self::default();
        let Some(help) = root.children.iter().find(|(tag, _)| tag == "help") else { return Ok(book) };
        for (tag, child) in &help.1.children {
            let id = child.attribute("id").unwrap_or("");
            match tag.as_str() {
                "toplevel" => book.root = section(child),
                "section" => {
                    book.sections.insert(id.into(), section(child));
                }
                "topic" => {
                    book.topics.insert(
                        id.into(),
                        Topic {
                            title: child.attribute("title").unwrap_or(id).into(),
                            spans: markup(child.attribute("text").unwrap_or("")),
                        },
                    );
                }
                _ => {}
            }
        }
        Ok(book)
    }

    pub fn initial(&self) -> Option<&str> {
        self.topics
            .get_key_value("..introduction")
            .map(|(key, _)| key.as_str())
            .or_else(|| {
                self.rows(&self.sections.keys().cloned().collect())
                    .into_iter()
                    .find_map(|row| (!row.folder).then_some(row.id))
            })
    }

    pub fn rows<'a>(&'a self, expanded: &HashSet<String>) -> Vec<Row<'a>> {
        let mut rows = Vec::new();
        self.append(&self.root, expanded, &mut HashSet::new(), 0, &mut rows);
        rows
    }

    fn append<'a>(
        &'a self,
        section: &'a Section,
        expanded: &HashSet<String>,
        visited: &mut HashSet<&'a str>,
        depth: usize,
        rows: &mut Vec<Row<'a>>,
    ) {
        for id in &section.sections {
            if id.starts_with('.') || !visited.insert(id) {
                continue;
            }
            let Some(child) = self.sections.get(id) else {
                continue;
            };
            if child.sections.is_empty()
                && !child
                    .topics
                    .iter()
                    .any(|topic| self.topics.contains_key(topic))
            {
                visited.remove(id.as_str());
                continue;
            }
            rows.push(Row {
                id,
                depth,
                title: &child.title,
                folder: true,
            });
            if expanded.contains(id) {
                self.append(child, expanded, visited, depth + 1, rows);
            }
            visited.remove(id.as_str());
        }
        for id in &section.topics {
            if id.starts_with('.') {
                continue;
            }
            if let Some(topic) = self.topics.get(id) {
                rows.push(Row {
                    id,
                    depth,
                    title: &topic.title,
                    folder: false,
                });
            }
        }
    }

    pub fn reveal(&self, id: &str, expanded: &mut HashSet<String>) {
        // Expand only the path leading to the target of a cross-reference.
        fn path(book: &Book, section: &Section, id: &str, parents: &mut Vec<String>) -> bool {
            if section.topics.iter().any(|topic| topic == id) {
                return true;
            }
            for key in &section.sections {
                if parents.contains(key) {
                    continue;
                }
                if let Some(child) = book.sections.get(key) {
                    parents.push(key.clone());
                    if key == id.trim_start_matches("..") || path(book, child, id, parents) {
                        return true;
                    }
                    parents.pop();
                }
            }
            false
        }
        let mut parents = Vec::new();
        if path(self, &self.root, id, &mut parents) {
            expanded.extend(parents);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_section_order_hidden_overviews_and_inline_links_survive_loading() {
        let data = br#"[help]
[toplevel]
sections=introduction
[/toplevel]
[section]
id=introduction
title=Introduction
topics=..introduction,control
[/section]
[topic]
id=..introduction
title=Introduction
text="See <ref>dst='control' text='Unit control'</ref>."
[/topic]
[topic]
id=control
title=Unit control
text="<italic>text='Move'</italic> safely."
[/topic]
[/help]"#;
        let book = Book::parse(data).unwrap();
        assert_eq!(book.initial(), Some("..introduction"));
        assert_eq!(book.rows(&HashSet::new()).len(), 1);
        let mut expanded = HashSet::new();
        book.reveal("control", &mut expanded);
        let rows = book.rows(&expanded);
        assert_eq!(
            rows.iter().map(|r| (r.id, r.depth)).collect::<Vec<_>>(),
            [("introduction", 0), ("control", 1)]
        );
        assert_eq!(
            book.topics["..introduction"].spans[1],
            Span::Link("Unit control".into(), "control".into())
        );
    }
}
