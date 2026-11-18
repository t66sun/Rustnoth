//! Parse preprocessed Wesnoth Markup Language.

use std::collections::BTreeMap;
use std::fmt;
use std::ops::Range;

/// The document root or the contents of a tag. Children keep their input order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Node {
    pub attributes: BTreeMap<String, Value>,
    pub children: Vec<(String, Node)>,
}

impl Node {
    pub fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes.get(name).map(|value| value.text.as_str())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Value {
    pub text: String,
    pub translations: Vec<Translation>,
}

/// A translatable segment of the source text. `bytes` indexes `Value::text`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Translation {
    pub bytes: Range<usize>,
    pub domain: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Position {
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ParseError {
    /// One-based physical input line and byte column.
    pub position: Position,
    pub message: String,
}

impl ParseError {
    fn new(position: Position, message: impl Into<String>) -> Self {
        Self {
            position,
            message: message.into(),
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}: {}",
            self.position.line, self.position.column, self.message
        )
    }
}

impl std::error::Error for ParseError {}

/// Accepts UTF-8 WML and the inline metadata that Wesnoth's preprocessor emits.
pub fn parse(input: impl AsRef<[u8]>) -> Result<Node, ParseError> {
    let mut lexer = Lexer::new(input.as_ref());
    let mut stack = vec![Frame::new(String::new(), lexer.position)];

    loop {
        let Token { kind, position } = lexer.next()?;
        let current = stack.last_mut().unwrap();
        match kind {
            Kind::LineEnd => {}
            Kind::Word(first) => read_attributes(&mut lexer, first, position, &mut current.node)?,
            Kind::Byte(b'[') => {
                let (tag, name) = read_tag(&mut lexer)?;
                match tag {
                    Tag::Open => stack.push(Frame::new(name, position)),
                    Tag::Append => {
                        let mut frame = Frame::new(name, position);
                        if let Some(index) = current
                            .node
                            .children
                            .iter()
                            .rposition(|(child, _)| child == &frame.name)
                        {
                            frame.node = std::mem::take(&mut current.node.children[index].1);
                            frame.original_index = Some(index);
                        }
                        stack.push(frame);
                    }
                    Tag::Close => {
                        if current.name.is_empty() {
                            return Err(ParseError::new(position, "unexpected closing tag"));
                        }
                        if current.name != name {
                            return Err(ParseError::new(
                                position,
                                format!("expected [/{0}], found [/{name}]", current.name),
                            ));
                        }
                        let closed = stack.pop().unwrap();
                        let siblings = &mut stack.last_mut().unwrap().node.children;
                        if let Some(index) = closed.original_index {
                            siblings[index].1 = closed.node;
                        } else {
                            siblings.push((closed.name, closed.node));
                        }
                    }
                }
            }
            Kind::End if current.name.is_empty() => return Ok(stack.pop().unwrap().node),
            Kind::End => {
                return Err(ParseError::new(
                    current.opening,
                    format!("missing closing tag [/{0}]", current.name),
                ));
            }
            _ => return Err(ParseError::new(position, "expected a tag or attribute")),
        }
    }
}

struct Frame {
    name: String,
    node: Node,
    opening: Position,
    original_index: Option<usize>,
}

impl Frame {
    fn new(name: String, opening: Position) -> Self {
        Self {
            name,
            node: Node::default(),
            opening,
            original_index: None,
        }
    }
}

enum Tag {
    Open,
    Append,
    Close,
}

fn read_tag(lexer: &mut Lexer<'_>) -> Result<(Tag, String), ParseError> {
    let first = lexer.next()?;
    let (tag, name) = match first.kind {
        Kind::Byte(b'+') => (Tag::Append, lexer.next()?),
        Kind::Byte(b'/') => (Tag::Close, lexer.next()?),
        _ => (Tag::Open, first),
    };
    let Kind::Word(name_text) = name.kind else {
        return Err(ParseError::new(name.position, "expected a tag name"));
    };
    let end = lexer.next()?;
    if !matches!(end.kind, Kind::Byte(b']')) {
        return Err(ParseError::new(end.position, "expected ']'"));
    }
    Ok((tag, name_text))
}

fn read_attributes(
    lexer: &mut Lexer<'_>,
    first: String,
    position: Position,
    node: &mut Node,
) -> Result<(), ParseError> {
    let mut names = Vec::new();
    let mut name = first;
    loop {
        let token = lexer.next()?;
        match token.kind {
            Kind::Word(word) => {
                if !name.is_empty() {
                    name.push(' ');
                }
                name.push_str(&word);
            }
            Kind::Byte(separator @ (b',' | b'=')) => {
                if name.is_empty() {
                    return Err(ParseError::new(token.position, "empty attribute name"));
                }
                names.push(std::mem::take(&mut name));
                if separator == b'=' {
                    break;
                }
            }
            _ => return Err(ParseError::new(token.position, "expected ',' or '='")),
        }
    }

    let mut names = names.into_iter().peekable();
    let mut name = names.next().unwrap();
    let mut value = Vec::new();
    let mut translations = Vec::new();
    let mut previous_word = false;
    let mut continued = false;
    loop {
        let token = lexer.next()?;
        previous_word = match token.kind {
            Kind::End => break,
            Kind::LineEnd if continued => continue,
            Kind::LineEnd => break,
            Kind::Byte(b'+') => {
                continued = true;
                continue;
            }
            Kind::Byte(b',') if names.peek().is_some() => {
                store_value(node, name, value, translations, position)?;
                name = names.next().unwrap();
                value = Vec::new();
                translations = Vec::new();
                false
            }
            Kind::Byte(b'_') => match lexer.next()?.kind {
                Kind::Quoted(quoted) => {
                    let start = value.len();
                    value.extend(quoted);
                    translations.push(Translation {
                        bytes: start..value.len(),
                        domain: lexer.domain.clone(),
                    });
                    false
                }
                Kind::LineEnd | Kind::End => {
                    value.push(b'_');
                    break;
                }
                Kind::Word(word) => {
                    value.push(b'_');
                    value.extend(word.bytes());
                    true
                }
                Kind::Byte(byte) => {
                    value.extend([b'_', byte]);
                    false
                }
            },
            Kind::Word(word) => {
                if previous_word {
                    value.push(b' ');
                }
                value.extend(word.bytes());
                true
            }
            Kind::Quoted(quoted) => {
                value.extend(quoted);
                false
            }
            Kind::Byte(byte) => {
                value.push(byte);
                false
            }
        };
        continued = false;
    }

    store_value(node, name, value, translations, position)?;
    for name in names {
        node.attributes.insert(name, Value::default());
    }
    Ok(())
}

fn store_value(
    node: &mut Node,
    name: String,
    bytes: Vec<u8>,
    translations: Vec<Translation>,
    position: Position,
) -> Result<(), ParseError> {
    let text = String::from_utf8(bytes)
        .map_err(|_| ParseError::new(position, "attribute value is not valid UTF-8"))?;
    node.attributes.insert(name, Value { text, translations });
    Ok(())
}

struct Token {
    kind: Kind,
    position: Position,
}

enum Kind {
    Word(String),
    Quoted(Vec<u8>),
    Byte(u8),
    LineEnd,
    End,
}

// The preprocessor marks metadata lines with 0xFE so they can appear inside a token.
const PREPROCESSOR_MARKER: u8 = 0xFE;

struct Lexer<'a> {
    remaining: &'a [u8],
    position: Position,
    domain: String,
}

impl<'a> Lexer<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        let mut lexer = Self {
            remaining: bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes),
            position: Position { line: 1, column: 1 },
            domain: "wesnoth".to_owned(),
        };
        lexer.skip_carriage_returns();
        lexer
    }

    fn peek(&self) -> Option<u8> {
        self.remaining.first().copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let (&byte, rest) = self.remaining.split_first()?;
        self.remaining = rest;
        if byte == b'\n' {
            self.position.line += 1;
            self.position.column = 1;
        } else {
            self.position.column += 1;
        }
        self.skip_carriage_returns();
        Some(byte)
    }

    fn skip_carriage_returns(&mut self) {
        while let Some(rest) = self.remaining.strip_prefix(b"\r") {
            self.remaining = rest;
            self.position.column += 1;
        }
    }

    fn skip_comment(&mut self) {
        let inline = self.bump() == Some(PREPROCESSOR_MARKER);
        let mut text = Vec::new();
        while self.peek().is_some_and(|byte| byte != b'\n') {
            text.push(self.bump().unwrap());
        }
        if let Some([b' ' | b'\t', domain @ ..]) = text.strip_prefix(b"textdomain") {
            self.domain = String::from_utf8_lossy(domain).into_owned();
        }
        if inline {
            self.bump();
        }
    }

    fn next(&mut self) -> Result<Token, ParseError> {
        loop {
            match self.peek() {
                Some(b' ' | b'\t') => {
                    self.bump();
                }
                Some(b'#' | PREPROCESSOR_MARKER) => self.skip_comment(),
                _ => break,
            }
        }

        let position = self.position;
        let kind = match self.bump() {
            None => Kind::End,
            Some(b'\n') => Kind::LineEnd,
            Some(b'"') => Kind::Quoted(self.quoted(b'"', position)?),
            Some(b'<') if self.peek() == Some(b'<') => {
                self.bump();
                Kind::Quoted(self.quoted(b'>', position)?)
            }
            Some(b'_') if !self.peek().is_some_and(is_alphanumeric) => Kind::Byte(b'_'),
            Some(byte) if is_word_byte(byte) => Kind::Word(self.word(byte)),
            Some(byte) => Kind::Byte(byte),
        };
        Ok(Token { kind, position })
    }

    fn word(&mut self, first: u8) -> String {
        let mut word = String::from(char::from(first));
        while let Some(byte) = self.peek() {
            if byte == PREPROCESSOR_MARKER {
                self.skip_comment();
            } else if is_word_byte(byte) {
                self.bump();
                word.push(char::from(byte));
            } else {
                break;
            }
        }
        word
    }

    fn quoted(&mut self, delimiter: u8, start: Position) -> Result<Vec<u8>, ParseError> {
        let mut value = Vec::new();
        while let Some(byte) = self.peek() {
            if byte == PREPROCESSOR_MARKER && delimiter == b'"' {
                self.skip_comment();
                continue;
            }
            self.bump();
            if byte == delimiter {
                let doubled = self.peek() == Some(delimiter);
                if doubled {
                    self.bump();
                }
                if doubled == (delimiter == b'>') {
                    return Ok(value);
                }
            }
            value.push(byte);
        }
        Err(ParseError::new(start, "unterminated quoted string"))
    }
}

fn is_alphanumeric(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn is_word_byte(byte: u8) -> bool {
    is_alphanumeric(byte) || byte == b'$'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_to_the_last_matching_child_at_its_original_position() {
        let root = parse(
            "[scenario]\n[unit]\nid=first\n[/unit]\n[unit]\nid=second\n[/unit]\n\
             [side][/side]\n[+unit]\n[attack][/attack]\nid=updated\n[/unit]\n\
             [+event][/event]\n[/scenario]",
        )
        .unwrap();
        let (tag, scenario) = &root.children[0];
        assert_eq!(tag, "scenario");
        assert_eq!(
            scenario
                .children
                .iter()
                .map(|(name, node)| (name.as_str(), node.attribute("id")))
                .collect::<Vec<_>>(),
            [
                ("unit", Some("first")),
                ("unit", Some("updated")),
                ("side", None),
                ("event", None),
            ]
        );
        assert_eq!(scenario.children[1].1.children[0].0, "attack");
    }

    #[test]
    fn reads_quoted_and_multiple_assignment_values() {
        let root = parse(
            "\u{feff}name = Press  here # comment\r\n\
             label = \"say \"\"hello\"\"\" +\r\n\" 世界\"\r\n\
             code = <<[x]# literal\r\n[/x]>>\r\n\
             first,second,third = foo,\"a,b\",bar,baz\r\n\
             provided,missing = yes",
        )
        .unwrap();
        assert_eq!(root.attribute("name"), Some("Press here"));
        assert_eq!(root.attribute("label"), Some("say \"hello\" 世界"));
        assert_eq!(root.attribute("code"), Some("[x]# literal\n[/x]"));
        assert_eq!(root.attribute("first"), Some("foo"));
        assert_eq!(root.attribute("second"), Some("a,b"));
        assert_eq!(root.attribute("third"), Some("bar,baz"));
        assert_eq!(root.attribute("missing"), Some(""));
    }

    #[test]
    fn keeps_domains_and_joins_tokens_split_by_preprocessor_metadata() {
        let root = parse(
            b"\xFEline 10 scenario.cfg\n#textdomain wesnoth-test\n\
              guide=uni\xFEline 4 macro.cfg\nted\n\
              label=_ \"hello\" +\n\xFEtextdomain wesnoth-other\n_ \"world\" + \"!\"\n\
              inside=\"one\xFEline 10 macro.cfg\ntwo\"",
        )
        .unwrap();
        assert_eq!(root.attribute("guide"), Some("united"));
        assert_eq!(root.attribute("inside"), Some("onetwo"));
        let label = &root.attributes["label"];
        assert_eq!(label.text, "helloworld!");
        assert_eq!(
            label.translations,
            [
                Translation {
                    bytes: 0..5,
                    domain: "wesnoth-test".into(),
                },
                Translation {
                    bytes: 5..10,
                    domain: "wesnoth-other".into(),
                },
            ]
        );
    }

    #[test]
    fn reports_the_location_of_invalid_wml() {
        let error = parse("[scenario]\n  [/unit]\n").unwrap_err();
        assert_eq!(error.position, Position { line: 2, column: 3 });
        assert!(error.message.contains("[/scenario]"));

        let error = parse("name=\"unterminated").unwrap_err();
        assert_eq!(error.position, Position { line: 1, column: 6 });
        assert_eq!(error.message, "unterminated quoted string");
    }
}
