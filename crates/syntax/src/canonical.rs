//! Canonical prints: a value as text that shows every field it has and
//! depends on nothing but the value - no address, node id, interner index
//! or hash order - so that two runs, or two builds, print one value alike.
//! The dumps that compare one build's front end, registry and checker with
//! another's are made of them.
//!
//! A print is a tree: an atom (a number, a name, a quoted string, a type)
//! stays on its line; a node puts each of its fields, and a list each of
//! its items, on a line of its own, indented under it.

#![deny(clippy::wildcard_enum_match_arm)]

use std::fmt::Write as _;
use std::sync::Arc;

use crate::name::Name;
use crate::php_string::PhpString;
use crate::span::Span;

/// A value with a canonical print. An implementation names every field of
/// the value, destructuring without `..` and matching without a `_` arm,
/// so that a field added to the value cannot be left out of its print
/// unseen.
pub trait Canonical {
    fn canonical(&self, p: &mut Printer);
}

/// `value`'s canonical print.
pub fn print(value: &(impl Canonical + ?Sized)) -> String {
    let mut p = Printer::default();
    value.canonical(&mut p);
    p.out
}

/// Where a canonical print is written.
#[derive(Default)]
pub struct Printer {
    out: String,
    depth: usize,
}

impl Printer {
    /// Writes `text` on the current line.
    pub fn atom(&mut self, text: &str) {
        self.out.push_str(text);
    }

    /// Writes `bytes` quoted (`quote_bytes`).
    pub fn bytes(&mut self, bytes: &[u8]) {
        quote_bytes(bytes, &mut self.out);
    }

    /// Writes the name `name` (`write_name`).
    pub fn name(&mut self, name: &str) {
        write_name(name, &mut self.out);
    }

    /// Writes a node of `kind` with `fields`, each `name: value` on a line
    /// of its own.
    pub fn node(&mut self, kind: &str, fields: &[(&str, &dyn Canonical)]) {
        self.out.push_str(kind);
        if fields.is_empty() {
            self.out.push_str(" {}");
            return;
        }
        self.out.push_str(" {");
        self.depth += 1;
        for (name, value) in fields {
            self.newline();
            self.out.push_str(name);
            self.out.push_str(": ");
            value.canonical(self);
        }
        self.depth -= 1;
        self.newline();
        self.out.push('}');
    }

    /// Writes `items` in their order, each on a line of its own.
    pub fn list<'a, T: Canonical + ?Sized + 'a>(&mut self, items: impl IntoIterator<Item = &'a T>) {
        let printed: Vec<String> = items.into_iter().map(|item| self.nested(item)).collect();
        self.lines(printed);
    }

    /// Writes `items` sorted by their prints: a set, whose order is not
    /// part of the value.
    pub fn set<'a, T: Canonical + ?Sized + 'a>(&mut self, items: impl IntoIterator<Item = &'a T>) {
        let mut printed: Vec<String> = items.into_iter().map(|item| self.nested(item)).collect();
        printed.sort_unstable();
        self.lines(printed);
    }

    /// Writes `entries` sorted by their keys' prints, each `key => value`
    /// on a line of its own: a map, whose order is not part of the value.
    pub fn map<'a, K, V>(&mut self, entries: impl IntoIterator<Item = (&'a K, &'a V)>)
    where
        K: Canonical + ?Sized + 'a,
        V: Canonical + ?Sized + 'a,
    {
        let mut printed: Vec<(String, String)> = entries
            .into_iter()
            .map(|(key, value)| (self.nested(key), self.nested(value)))
            .collect();
        printed.sort_unstable();
        self.lines(
            printed
                .into_iter()
                .map(|(key, value)| format!("{key} => {value}"))
                .collect(),
        );
    }

    /// `value` printed one level below the current line.
    fn nested(&self, value: &(impl Canonical + ?Sized)) -> String {
        let mut p = Printer {
            out: String::new(),
            depth: self.depth + 1,
        };
        value.canonical(&mut p);
        p.out
    }

    fn lines(&mut self, printed: Vec<String>) {
        if printed.is_empty() {
            self.out.push_str("[]");
            return;
        }
        self.out.push('[');
        self.depth += 1;
        for line in printed {
            self.newline();
            self.out.push_str(&line);
        }
        self.depth -= 1;
        self.newline();
        self.out.push(']');
    }

    fn newline(&mut self) {
        self.out.push('\n');
        for _ in 0..self.depth {
            self.out.push_str("  ");
        }
    }
}

/// Writes `bytes` between single quotes: each character as it is, but `\`
/// and `'` escaped, the bytes of a character `unseen` as `\xNN`, and so
/// every byte outside UTF-8.
pub fn quote_bytes(bytes: &[u8], out: &mut String) {
    out.push('\'');
    for chunk in bytes.utf8_chunks() {
        for ch in chunk.valid().chars() {
            match ch {
                '\\' => out.push_str("\\\\"),
                '\'' => out.push_str("\\'"),
                ch if unseen(ch) => {
                    let mut buf = [0; 4];
                    for byte in ch.encode_utf8(&mut buf).bytes() {
                        let _ = write!(out, "\\x{byte:02x}");
                    }
                }
                ch => out.push(ch),
            }
        }
        for byte in chunk.invalid() {
            let _ = write!(out, "\\x{byte:02x}");
        }
    }
    out.push('\'');
}

/// Writes the name `name` as it is spelled, but the bytes of a character
/// `unseen` and of the space as `\x{NN}`, which no name's text can be.
pub fn write_name(name: &str, out: &mut String) {
    for ch in name.chars() {
        if unseen(ch) || ch == ' ' {
            let mut buf = [0; 4];
            for byte in ch.encode_utf8(&mut buf).bytes() {
                let _ = write!(out, "\\x{{{byte:02x}}}");
            }
        } else {
            out.push(ch);
        }
    }
}

/// Whether `ch` cannot be told from another character, or from none, as
/// it is: a control character, or whitespace other than the space.
fn unseen(ch: char) -> bool {
    ch.is_control() || (ch.is_whitespace() && ch != ' ')
}

impl<T: Canonical + ?Sized> Canonical for &T {
    fn canonical(&self, p: &mut Printer) {
        (**self).canonical(p);
    }
}

impl<T: Canonical + ?Sized> Canonical for Box<T> {
    fn canonical(&self, p: &mut Printer) {
        (**self).canonical(p);
    }
}

impl<T: Canonical + ?Sized> Canonical for Arc<T> {
    fn canonical(&self, p: &mut Printer) {
        (**self).canonical(p);
    }
}

/// `-` for `None`, which no other print is.
impl<T: Canonical> Canonical for Option<T> {
    fn canonical(&self, p: &mut Printer) {
        match self {
            Some(value) => value.canonical(p),
            None => p.atom("-"),
        }
    }
}

impl<T: Canonical> Canonical for [T] {
    fn canonical(&self, p: &mut Printer) {
        p.list(self);
    }
}

impl<T: Canonical> Canonical for Vec<T> {
    fn canonical(&self, p: &mut Printer) {
        p.list(self.as_slice());
    }
}

impl<A: Canonical, B: Canonical> Canonical for (A, B) {
    fn canonical(&self, p: &mut Printer) {
        let (a, b) = self;
        p.atom("(");
        a.canonical(p);
        p.atom(", ");
        b.canonical(p);
        p.atom(")");
    }
}

impl Canonical for bool {
    fn canonical(&self, p: &mut Printer) {
        p.atom(if *self { "true" } else { "false" });
    }
}

macro_rules! canonical_integers {
    ($($int:ty),*) => {$(
        impl Canonical for $int {
            fn canonical(&self, p: &mut Printer) {
                p.atom(&self.to_string());
            }
        }
    )*};
}

canonical_integers!(u8, u16, u32, u64, usize, i8, i16, i32, i64, isize);

/// A string is quoted: it is text, not a name.
impl Canonical for str {
    fn canonical(&self, p: &mut Printer) {
        p.bytes(self.as_bytes());
    }
}

impl Canonical for String {
    fn canonical(&self, p: &mut Printer) {
        p.bytes(self.as_bytes());
    }
}

impl Canonical for PhpString {
    fn canonical(&self, p: &mut Printer) {
        p.bytes(self.as_bytes());
    }
}

impl Canonical for Name {
    fn canonical(&self, p: &mut Printer) {
        p.name(self.as_str());
    }
}

impl Canonical for Span {
    fn canonical(&self, p: &mut Printer) {
        let Span { start, end } = self;
        p.atom(&format!("{start}..{end}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Pair {
        left: u32,
        right: Vec<Name>,
    }

    impl Canonical for Pair {
        fn canonical(&self, p: &mut Printer) {
            let Pair { left, right } = self;
            p.node("Pair", &[("left", left), ("right", right)]);
        }
    }

    #[test]
    fn a_node_puts_each_field_and_a_list_each_item_on_a_line_of_its_own() {
        let pair = Pair {
            left: 1,
            right: vec![Name::from("B"), Name::from("A")],
        };
        assert_eq!(
            print(&pair),
            "Pair {\n  left: 1\n  right: [\n    B\n    A\n  ]\n}"
        );
        let nested = vec![Pair {
            left: 2,
            right: vec![],
        }];
        assert_eq!(
            print(&nested),
            "[\n  Pair {\n    left: 2\n    right: []\n  }\n]"
        );
    }

    #[test]
    fn a_set_and_a_map_print_sorted() {
        let mut p = Printer::default();
        p.set(&[Name::from("b"), Name::from("a")]);
        p.atom(" ");
        p.map([(&2u32, &true), (&1u32, &false)]);
        assert_eq!(p.out, "[\n  a\n  b\n] [\n  1 => false\n  2 => true\n]");
    }

    #[test]
    fn quoted_bytes_escape_what_is_not_printable() {
        let mut out = String::new();
        quote_bytes(b"it's \\ \x7f\xe2\x80\xa8\xc3\xa9\xfe", &mut out);
        assert_eq!(out, r"'it\'s \\ \x7f\xe2\x80\xa8é\xfe'");
        let mut name = String::new();
        write_name("A\u{1}B", &mut name);
        assert_eq!(name, r"A\x{01}B");
    }
}
