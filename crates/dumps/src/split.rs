//! How a dump's text is cut into entities as it is written.
//!
//! **By prefix:** each run of lines alike up to the separator is an entity,
//! named by what precedes it.
//!
//! **As a tree:** the text is canonical prints (`syntax::canonical`) in
//! sections, each a line `== name` with its print below it, and a line's
//! indent is what it nests in. The text before the first section is the
//! root's own. A section, and a field (`name: value`), is a directory when
//! one of its parts spans lines, and an entity otherwise:
//! - a section's parts are the items and fields its print is made of, or,
//!   when its print is one node or list over several lines, that node's or
//!   list's: its first line and its closing lines are then the section's
//!   own text;
//! - a field's parts are its value's fields, entries and items, its first
//!   and closing lines its own text;
//! - an entry (`key => value`) and an item are each an entity, every line
//!   it spans, and a field is one or a directory again.
//!
//! A directory's own text is entities named `.0`, `.1` and so on, one for
//! each run of its own lines. A section is named by its name, a field by
//! its name, an entry by its key, an item spanning lines by the value of
//! its second line when that is a field, any other by its line; a name
//! repeated in a directory takes `~2`, `~3` and so on after it (`escape`).
//! A line holding only spaces belongs with the line before it.
//!
//! Each byte goes to one entity, in order: the entities' texts in the
//! manifest's order are the text, whatever it holds, and the way it is
//! printed only decides where the cuts fall.

use std::io;

use rustc_hash::FxHashMap;

use crate::manifest::Kind;
use crate::name::escape;
use crate::tree::join;

/// Where the entities go, in the text's order.
pub(crate) trait Emit {
    fn entity(&mut self, path: &str, text: &[u8]) -> io::Result<()>;
}

/// Cuts a text given in pieces of any size.
pub(crate) struct Splitter {
    /// The text after the last whole line.
    partial: Vec<u8>,
    cut: Cut,
}

enum Cut {
    Tree(TreeCut),
    Prefix(PrefixCut),
}

impl Splitter {
    /// The splitter of the dumps of `kind`, which a dump of files has none
    /// of.
    pub(crate) fn new(kind: Kind) -> Option<Splitter> {
        let cut = match kind {
            Kind::Tree => Cut::Tree(TreeCut {
                frames: vec![Frame::new(String::new(), Role::Root, None)],
                open: Open::Nothing,
            }),
            Kind::ByPrefix(separator) => Cut::Prefix(PrefixCut {
                separator,
                key: Vec::new(),
                text: Vec::new(),
                names: Names::default(),
            }),
            Kind::Files => return None,
        };
        Some(Splitter {
            partial: Vec::new(),
            cut,
        })
    }

    pub(crate) fn push(&mut self, mut bytes: &[u8], emit: &mut dyn Emit) -> io::Result<()> {
        if !self.partial.is_empty() {
            let Some(end) = memchr::memchr(b'\n', bytes) else {
                self.partial.extend_from_slice(bytes);
                return Ok(());
            };
            let mut line = std::mem::take(&mut self.partial);
            line.extend_from_slice(&bytes[..=end]);
            self.line(&line, emit)?;
            line.clear();
            self.partial = line;
            bytes = &bytes[end + 1..];
        }
        while let Some(end) = memchr::memchr(b'\n', bytes) {
            self.line(&bytes[..=end], emit)?;
            bytes = &bytes[end + 1..];
        }
        self.partial.extend_from_slice(bytes);
        Ok(())
    }

    /// Cuts what is left: a last line without its line break, and what is
    /// still open.
    pub(crate) fn finish(mut self, emit: &mut dyn Emit) -> io::Result<()> {
        if !self.partial.is_empty() {
            let line = std::mem::take(&mut self.partial);
            self.line(&line, emit)?;
        }
        match &mut self.cut {
            Cut::Tree(cut) => cut.end_open(emit),
            Cut::Prefix(cut) => cut.end(emit),
        }
    }

    fn line(&mut self, line: &[u8], emit: &mut dyn Emit) -> io::Result<()> {
        match &mut self.cut {
            Cut::Tree(cut) => cut.line(line, emit),
            Cut::Prefix(cut) => cut.line(line, emit),
        }
    }
}

/// The names given in a directory, and how often each.
#[derive(Default)]
struct Names(FxHashMap<String, u32>);

impl Names {
    /// The name of the next entity whose key escapes to `escaped`.
    fn name(&mut self, escaped: String) -> String {
        let Names(given) = self;
        match given.get_mut(&escaped) {
            Some(count) => {
                *count += 1;
                format!("{escaped}~{count}")
            }
            None => {
                given.insert(escaped.clone(), 1);
                escaped
            }
        }
    }
}

struct PrefixCut {
    separator: u8,
    key: Vec<u8>,
    text: Vec<u8>,
    names: Names,
}

impl PrefixCut {
    fn line(&mut self, line: &[u8], emit: &mut dyn Emit) -> io::Result<()> {
        let content = line.strip_suffix(b"\n").unwrap_or(line);
        let key = memchr::memchr(self.separator, content).map_or(content, |end| &content[..end]);
        if !self.text.is_empty() && key != self.key.as_slice() {
            self.end(emit)?;
        }
        if self.text.is_empty() {
            self.key.clear();
            self.key.extend_from_slice(key);
        }
        self.text.extend_from_slice(line);
        Ok(())
    }

    fn end(&mut self, emit: &mut dyn Emit) -> io::Result<()> {
        if self.text.is_empty() {
            return Ok(());
        }
        let name = self.names.name(escape(&self.key));
        emit.entity(&name, &self.text)?;
        self.text.clear();
        Ok(())
    }
}

/// What a line is, for cutting.
#[derive(Clone, Copy)]
struct Shape {
    indent: usize,
    /// Only spaces.
    blank: bool,
    /// Starts, after its indent, with `}` or `]`.
    closer: bool,
    /// Starts a section.
    section: bool,
}

impl Shape {
    fn of(line: &[u8]) -> Shape {
        let content = line.strip_suffix(b"\n").unwrap_or(line);
        let indent = content.iter().take_while(|&&byte| byte == b' ').count();
        let rest = &content[indent..];
        Shape {
            indent,
            blank: rest.is_empty(),
            closer: matches!(rest.first(), Some(b'}' | b']')),
            section: indent == 0 && rest.starts_with(b"== "),
        }
    }
}

struct TreeCut {
    /// The root, then each directory open in it.
    frames: Vec<Frame>,
    /// What the lines go to, in the innermost directory.
    open: Open,
}

/// A directory being cut.
struct Frame {
    path: String,
    role: Role,
    /// The indent of its own closing lines, once known.
    indent: Option<usize>,
    /// How many runs of its own lines it has had.
    own: u32,
    names: Names,
}

impl Frame {
    fn new(path: String, role: Role, indent: Option<usize>) -> Frame {
        Frame {
            path,
            role,
            indent,
            own: 0,
            names: Names::default(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Root,
    /// A section, and whether its first part has decided how it is cut.
    Section {
        decided: bool,
    },
    /// A field cut as its value's content.
    Field,
}

enum Open {
    Nothing,
    /// A run of the directory's own lines.
    Own(Vec<u8>),
    Child(Child),
}

/// A section, a field, an entry or an item, with the lines it spans so
/// far.
struct Child {
    indent: usize,
    text: Vec<u8>,
    /// The length of its first line.
    first: usize,
    kind: ChildKind,
    /// Its lines but the blank ones.
    lines: u32,
    /// An item's second line's value, when that line is a field.
    second: Option<Vec<u8>>,
    /// A section's or a field's: the indent its latest part started at,
    /// while one is open.
    part: Option<usize>,
    /// A section's: how its print is read so far.
    scan: Scan,
}

enum ChildKind {
    Section(Vec<u8>),
    Field(Vec<u8>),
    Entry(Vec<u8>),
    Item,
}

/// How a section's print is read, to tell whether a part of it spans
/// lines.
#[derive(Clone, Copy)]
enum Scan {
    /// Nothing below the section's line yet.
    Head,
    /// One line of print, and whether it is an item.
    First { item: bool },
    /// Its parts: the content of a node or list from the indent of its
    /// closing lines, or the print's own items and fields.
    Parts { closing: Option<usize> },
}

impl Child {
    fn new(line: &[u8], shape: Shape) -> Child {
        let kind = if shape.section {
            let content = line.strip_suffix(b"\n").unwrap_or(line);
            ChildKind::Section(content[3..].to_vec())
        } else {
            kind_of(&line[shape.indent..])
        };
        Child {
            indent: shape.indent,
            text: line.to_vec(),
            first: line.len(),
            kind,
            lines: 1,
            second: None,
            part: None,
            scan: Scan::Head,
        }
    }

    /// Whether the line shaped `shape` is part of it: any line before the
    /// next section, for a section; one nested under it, or a closing line
    /// at its indent, for any other.
    fn continued_by(&self, shape: Shape) -> bool {
        !shape.section
            && (matches!(self.kind, ChildKind::Section(_))
                || shape.indent > self.indent
                || (shape.indent == self.indent && shape.closer))
    }

    /// Whether `line`, which is part of it, makes a section or a field a
    /// directory: whether it continues a part of it.
    fn spans_parts(&mut self, line: &[u8], shape: Shape) -> bool {
        let continues = |part: Option<usize>| {
            part.is_some_and(|at| shape.indent > at || (shape.indent == at && shape.closer))
        };
        match self.kind {
            ChildKind::Field(_) => {
                if shape.indent > self.indent && continues(self.part) {
                    return true;
                }
                self.part = (shape.indent > self.indent).then_some(shape.indent);
                false
            }
            ChildKind::Section(_) => match self.scan {
                Scan::Head => {
                    let item = matches!(kind_of(&line[shape.indent..]), ChildKind::Item);
                    self.scan = Scan::First { item };
                    self.part = Some(shape.indent);
                    false
                }
                Scan::First { item } => {
                    if !continues(self.part) {
                        self.scan = Scan::Parts { closing: None };
                        self.part = Some(shape.indent);
                        return false;
                    }
                    if !item {
                        return true;
                    }
                    self.scan = Scan::Parts { closing: self.part };
                    self.part = None;
                    self.spans_parts(line, shape)
                }
                Scan::Parts { closing } => {
                    if closing == Some(shape.indent) && shape.closer {
                        self.part = None;
                        return false;
                    }
                    if continues(self.part) {
                        return true;
                    }
                    self.part = Some(shape.indent);
                    false
                }
            },
            ChildKind::Entry(_) | ChildKind::Item => false,
        }
    }

    fn push(&mut self, line: &[u8], shape: Shape) {
        self.text.extend_from_slice(line);
        self.lines += 1;
        if self.lines == 2
            && matches!(self.kind, ChildKind::Item)
            && let ChildKind::Field(_) = kind_of(&line[shape.indent..])
        {
            let content = line.strip_suffix(b"\n").unwrap_or(line);
            let value =
                memchr::memmem::find(content, b": ").map_or(&[][..], |at| &content[at + 2..]);
            self.second = Some(value.to_vec());
        }
    }

    fn key(&self) -> &[u8] {
        match &self.kind {
            ChildKind::Section(name) | ChildKind::Field(name) => name,
            ChildKind::Entry(key) => key,
            ChildKind::Item => match &self.second {
                Some(value) => value,
                None => self.text[self.indent..self.first].trim_ascii_end(),
            },
        }
    }
}

/// What a line starts, from its text after its indent: a field when it
/// starts with a name and `: `, an entry when it has ` => ` outside quotes,
/// an item otherwise.
fn kind_of(content: &[u8]) -> ChildKind {
    let name_len = content
        .iter()
        .enumerate()
        .take_while(|&(at, &byte)| {
            byte == b'_' || byte.is_ascii_alphabetic() || (at > 0 && byte.is_ascii_digit())
        })
        .count();
    if name_len > 0 && content[name_len..].starts_with(b": ") {
        return ChildKind::Field(content[..name_len].to_vec());
    }
    let mut quoted = false;
    let mut at = 0;
    while at < content.len() {
        match content[at] {
            b'\\' if quoted => at += 1,
            b'\'' => quoted = !quoted,
            b' ' if !quoted && content[at..].starts_with(b" => ") => {
                return ChildKind::Entry(content[..at].to_vec());
            }
            _ => {}
        }
        at += 1;
    }
    ChildKind::Item
}

impl TreeCut {
    fn line(&mut self, line: &[u8], emit: &mut dyn Emit) -> io::Result<()> {
        let shape = Shape::of(line);
        if shape.blank {
            match &mut self.open {
                Open::Nothing => self.open = Open::Own(line.to_vec()),
                Open::Own(text) => text.extend_from_slice(line),
                Open::Child(child) => child.text.extend_from_slice(line),
            }
            return Ok(());
        }
        if let Open::Child(child) = &mut self.open {
            if child.continued_by(shape) {
                let frame = self.frames.last_mut().expect("the root stays");
                if frame.role == (Role::Section { decided: false }) {
                    // The section's first part, an item, spans lines: the
                    // section is cut as its content.
                    frame.role = Role::Section { decided: true };
                    frame.indent = Some(child.indent);
                    let Open::Child(child) = std::mem::replace(&mut self.open, Open::Nothing)
                    else {
                        unreachable!("matched above");
                    };
                    self.open = Open::Own(child.text);
                    return self.line(line, emit);
                }
                if child.spans_parts(line, shape) {
                    return self.cut(line, emit);
                }
                child.push(line, shape);
                return Ok(());
            }
            self.end_open(emit)?;
        }
        self.place(line, shape, emit)
    }

    /// Starts with `line` what it starts in the innermost directory it is
    /// in, ending those it is past.
    fn place(&mut self, line: &[u8], shape: Shape, emit: &mut dyn Emit) -> io::Result<()> {
        loop {
            let frame = self.frames.last_mut().expect("the root stays");
            match frame.role {
                Role::Root if shape.section => return self.start_child(line, shape, emit),
                Role::Root => return self.own(line, emit),
                Role::Section { .. } | Role::Field if shape.section => self.end_frame(emit)?,
                Role::Section { .. } if frame.indent == Some(shape.indent) && shape.closer => {
                    return self.own(line, emit);
                }
                Role::Section { .. } => return self.start_child(line, shape, emit),
                Role::Field => {
                    let indent = frame.indent.expect("a field's directory has its indent");
                    if shape.indent == indent && shape.closer {
                        return self.own(line, emit);
                    }
                    if shape.indent <= indent {
                        self.end_frame(emit)?;
                    } else {
                        return self.start_child(line, shape, emit);
                    }
                }
            }
        }
    }

    fn own(&mut self, line: &[u8], emit: &mut dyn Emit) -> io::Result<()> {
        match &mut self.open {
            Open::Own(text) => text.extend_from_slice(line),
            Open::Nothing => self.open = Open::Own(line.to_vec()),
            Open::Child(_) => {
                self.end_open(emit)?;
                self.open = Open::Own(line.to_vec());
            }
        }
        Ok(())
    }

    fn start_child(&mut self, line: &[u8], shape: Shape, emit: &mut dyn Emit) -> io::Result<()> {
        self.end_open(emit)?;
        let child = Child::new(line, shape);
        let frame = self.frames.last_mut().expect("the root stays");
        if frame.role == (Role::Section { decided: false })
            && !matches!(child.kind, ChildKind::Item)
        {
            frame.role = Role::Section { decided: true };
        }
        self.open = Open::Child(child);
        Ok(())
    }

    /// Makes the open section or field a directory, its first line its own
    /// text, and cuts its other lines and `line` in it.
    fn cut(&mut self, line: &[u8], emit: &mut dyn Emit) -> io::Result<()> {
        let Open::Child(child) = std::mem::replace(&mut self.open, Open::Nothing) else {
            unreachable!("a section or a field is open");
        };
        let frame = self.frames.last_mut().expect("the root stays");
        let name = frame.names.name(escape(child.key()));
        let path = join(&frame.path, &name);
        let (role, indent) = match child.kind {
            ChildKind::Section(_) => (Role::Section { decided: false }, None),
            _ => (Role::Field, Some(child.indent)),
        };
        self.frames.push(Frame::new(path, role, indent));
        let (first, mut rest) = child.text.split_at(child.first);
        self.open = Open::Own(first.to_vec());
        while !rest.is_empty() {
            let end = memchr::memchr(b'\n', rest).map_or(rest.len(), |end| end + 1);
            self.line(&rest[..end], emit)?;
            rest = &rest[end..];
        }
        self.line(line, emit)
    }

    fn end_frame(&mut self, emit: &mut dyn Emit) -> io::Result<()> {
        self.end_open(emit)?;
        self.frames.pop();
        Ok(())
    }

    fn end_open(&mut self, emit: &mut dyn Emit) -> io::Result<()> {
        let frame = self.frames.last_mut().expect("the root stays");
        match std::mem::replace(&mut self.open, Open::Nothing) {
            Open::Nothing => Ok(()),
            Open::Own(text) => {
                let path = join(&frame.path, &format!(".{}", frame.own));
                frame.own += 1;
                emit.entity(&path, &text)
            }
            Open::Child(child) => {
                if let Role::Section { decided: false } = frame.role {
                    frame.role = Role::Section { decided: true };
                }
                let name = frame.names.name(escape(child.key()));
                emit.entity(&join(&frame.path, &name), &child.text)
            }
        }
    }
}
