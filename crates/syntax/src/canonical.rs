//! Canonical prints: a value as text that shows every field it has and
//! depends on nothing but the value - no address, node id, interner index
//! or hash order - so that two runs, or two builds, print one value alike.
//! The dumps that compare one build's front end, registry and checker with
//! another's are made of them.
//!
//! A print is a tree: an atom (a number, a name, a quoted string, a type)
//! and a tuple (`Kind(a, b)`) stay on their line; a node puts each of its
//! fields, and a list each of its items, on a line of its own, indented
//! under it.
//!
//! `canonical_struct!` and `canonical_enum!` write a print for a struct or
//! an enum from the list of its fields or variants, which must name every
//! one: the pattern they destructure with has no `..`, and the match no `_`
//! arm.

#![deny(clippy::wildcard_enum_match_arm)]

use std::io;
use std::sync::Arc;

use crate::ast::NodeArenas;
use crate::name::Name;
use crate::php_string::PhpString;
use crate::references::{DocFrame, DocSite};
use crate::span::Span;

mod tokens;
mod tree;

/// A value with a canonical print. An implementation names every field of
/// the value, destructuring without `..` and matching without a `_` arm,
/// so that a field added to the value cannot be left out of its print
/// unseen.
pub trait Canonical {
    fn canonical(&self, p: &mut Printer);

    /// Prints a value whose nested AST node ids belong to `arenas`.
    /// Ordinary values use their representation-independent print, while
    /// arena-aware AST values override this to follow ids through the owner.
    fn canonical_with(&self, p: &mut Printer, arenas: &NodeArenas) {
        let _ = arenas;
        self.canonical(p);
    }
}

/// `value`'s canonical print.
pub fn print(value: &(impl Canonical + ?Sized)) -> String {
    let mut p = Printer::default();
    value.canonical(&mut p);
    p.out
}

/// `value`'s canonical print on one line: each line break, with the
/// indentation after it, a space. Nodes and lists are bracketed and
/// strings quoted with their line breaks escaped, so the line tells the
/// same values apart the print does; it is what a dump that is sorted by
/// line holds.
pub fn print_line(value: &(impl Canonical + ?Sized)) -> String {
    let printed = print(value);
    let mut out = String::with_capacity(printed.len());
    let mut lines = printed.split('\n');
    if let Some(first) = lines.next() {
        out.push_str(first);
    }
    for line in lines {
        out.push(' ');
        out.push_str(line.trim_start_matches(' '));
    }
    out
}

/// Where a canonical print is written: into memory, or into a sink it is
/// passed on to a line at a time once it has gathered `SINK_CHUNK` bytes.
#[derive(Default)]
pub struct Printer<'s> {
    out: String,
    depth: usize,
    sink: Option<&'s mut dyn io::Write>,
    /// The sink's first error; nothing is written to it after.
    error: Option<io::Error>,
}

/// How much a printer writing into a sink gathers before passing it on.
const SINK_CHUNK: usize = 1 << 16;

impl<'s> Printer<'s> {
    /// A printer writing into `sink`; `close` writes what is left.
    pub fn to(sink: &'s mut dyn io::Write) -> Printer<'s> {
        Printer {
            out: String::with_capacity(SINK_CHUNK + 4096),
            depth: 0,
            sink: Some(sink),
            error: None,
        }
    }

    /// What has been written, of a printer writing into memory.
    pub fn finish(self) -> String {
        debug_assert!(
            self.sink.is_none(),
            "a printer writing into a sink is closed"
        );
        self.out
    }

    /// Writes what is left into the sink; its first error, if it had one.
    pub fn close(mut self) -> io::Result<()> {
        self.pass_on();
        match self.error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn pass_on(&mut self) {
        let Some(sink) = &mut self.sink else {
            return;
        };
        if self.error.is_none()
            && let Err(error) = sink.write_all(self.out.as_bytes())
        {
            self.error = Some(error);
        }
        self.out.clear();
    }

    /// Starts a new line at the current depth.
    pub fn line(&mut self) {
        self.newline();
    }

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

    /// Writes a node while resolving arena-backed fields through `arenas`.
    pub fn node_with(
        &mut self,
        kind: &str,
        fields: &[(&str, &dyn Canonical)],
        arenas: &NodeArenas,
    ) {
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
            value.canonical_with(self, arenas);
        }
        self.depth -= 1;
        self.newline();
        self.out.push('}');
    }

    /// Writes a tuple of `kind` with `values`, `Kind(a, b)`, on the current
    /// line; without a kind, `(a, b)`.
    pub fn tuple(&mut self, kind: &str, values: &[&dyn Canonical]) {
        self.out.push_str(kind);
        self.out.push('(');
        for (i, value) in values.iter().enumerate() {
            if i > 0 {
                self.out.push_str(", ");
            }
            value.canonical(self);
        }
        self.out.push(')');
    }

    /// Writes a tuple while resolving arena-backed fields through `arenas`.
    pub fn tuple_with(&mut self, kind: &str, values: &[&dyn Canonical], arenas: &NodeArenas) {
        self.out.push_str(kind);
        self.out.push('(');
        for (i, value) in values.iter().enumerate() {
            if i > 0 {
                self.out.push_str(", ");
            }
            value.canonical_with(self, arenas);
        }
        self.out.push(')');
    }

    /// Writes `items` in their order, each on a line of its own.
    pub fn list<'a, T: Canonical + ?Sized + 'a>(&mut self, items: impl IntoIterator<Item = &'a T>) {
        let mut items = items.into_iter().peekable();
        if items.peek().is_none() {
            self.out.push_str("[]");
            return;
        }
        self.out.push('[');
        self.depth += 1;
        for item in items {
            self.newline();
            item.canonical(self);
        }
        self.depth -= 1;
        self.newline();
        self.out.push(']');
    }

    /// Writes a list while resolving arena-backed items through `arenas`.
    pub fn list_with<'a, T: Canonical + ?Sized + 'a>(
        &mut self,
        items: impl IntoIterator<Item = &'a T>,
        arenas: &NodeArenas,
    ) {
        let mut items = items.into_iter().peekable();
        if items.peek().is_none() {
            self.out.push_str("[]");
            return;
        }
        self.out.push('[');
        self.depth += 1;
        for item in items {
            self.newline();
            item.canonical_with(self, arenas);
        }
        self.depth -= 1;
        self.newline();
        self.out.push(']');
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

    /// Writes `entries`, each `name => value` on a line of its own, sorted
    /// by the name in ASCII lowercase and then as written: a table by
    /// name, whose order is not part of it.
    pub fn by_name<'a, V: Canonical + ?Sized + 'a>(
        &mut self,
        entries: impl IntoIterator<Item = (&'a Name, &'a V)>,
    ) {
        let mut entries: Vec<(&Name, &V)> = entries.into_iter().collect();
        entries.sort_unstable_by(|(a, _), (b, _)| {
            (a.lower(), a.as_str()).cmp(&(b.lower(), b.as_str()))
        });
        if entries.is_empty() {
            self.out.push_str("[]");
            return;
        }
        self.out.push('[');
        self.depth += 1;
        for (name, value) in entries {
            self.newline();
            name.canonical(self);
            self.out.push_str(" => ");
            value.canonical(self);
        }
        self.depth -= 1;
        self.newline();
        self.out.push(']');
    }

    /// Writes `entries`, each `key => value` on a line of its own, in the
    /// order they come in: a map whose order the caller has fixed, written
    /// as its entries are made.
    pub fn map_in_order<K: Canonical, V: Canonical>(
        &mut self,
        entries: impl IntoIterator<Item = (K, V)>,
    ) {
        let mut entries = entries.into_iter().peekable();
        if entries.peek().is_none() {
            self.out.push_str("[]");
            return;
        }
        self.out.push('[');
        self.depth += 1;
        for (key, value) in entries {
            self.newline();
            key.canonical(self);
            self.out.push_str(" => ");
            value.canonical(self);
        }
        self.depth -= 1;
        self.newline();
        self.out.push(']');
    }

    /// `value` printed one level below the current line.
    fn nested(&self, value: &(impl Canonical + ?Sized)) -> String {
        let mut p = Printer {
            depth: self.depth + 1,
            ..Printer::default()
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
        if self.sink.is_some() && self.out.len() >= SINK_CHUNK {
            self.pass_on();
        }
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
        write_escaped(
            chunk.valid(),
            out,
            |ascii| matches!(ascii, b'\\' | b'\''),
            |ch, out| match ch {
                '\\' => out.push_str("\\\\"),
                '\'' => out.push_str("\\'"),
                ch => escape_char(ch, out, false),
            },
        );
        for &byte in chunk.invalid() {
            escape_byte(byte, out, false);
        }
    }
    out.push('\'');
}

/// Writes the name `name` as it is spelled, but the bytes of a character
/// `unseen` and of the space as `\x{NN}`, which no name's text can be.
pub fn write_name(name: &str, out: &mut String) {
    write_escaped(
        name,
        out,
        |ascii| ascii == b' ',
        |ch, out| {
            escape_char(ch, out, true);
        },
    );
}

/// Writes `text`, each character `unseen` and each ASCII one `special`
/// says through `escape`, and every run of the others copied whole.
fn write_escaped(
    text: &str,
    out: &mut String,
    special: impl Fn(u8) -> bool,
    escape: impl Fn(char, &mut String),
) {
    let bytes = text.as_bytes();
    let mut plain = 0;
    let mut at = 0;
    while at < bytes.len() {
        let byte = bytes[at];
        let ch = if byte.is_ascii() {
            if !special(byte) && !unseen_ascii(byte) {
                at += 1;
                continue;
            }
            char::from(byte)
        } else {
            let ch = text[at..].chars().next().expect("a character starts here");
            if !unseen(ch) {
                at += ch.len_utf8();
                continue;
            }
            ch
        };
        out.push_str(&text[plain..at]);
        escape(ch, out);
        at += ch.len_utf8();
        plain = at;
    }
    out.push_str(&text[plain..]);
}

/// Writes each byte of `ch` as `\xNN`, or as `\x{NN}` when `braced`.
fn escape_char(ch: char, out: &mut String, braced: bool) {
    let mut buf = [0; 4];
    for &byte in ch.encode_utf8(&mut buf).as_bytes() {
        escape_byte(byte, out, braced);
    }
}

/// Writes `byte` as `\xNN`, or as `\x{NN}` when `braced`.
fn escape_byte(byte: u8, out: &mut String, braced: bool) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.push_str(if braced { "\\x{" } else { "\\x" });
    out.push(char::from(HEX[usize::from(byte >> 4)]));
    out.push(char::from(HEX[usize::from(byte & 0xf)]));
    if braced {
        out.push('}');
    }
}

/// Whether `ch` cannot be told from another character, or from none, as
/// it is: a control character, or whitespace other than the space.
fn unseen(ch: char) -> bool {
    ch.is_control() || (ch.is_whitespace() && ch != ' ')
}

/// `unseen` for an ASCII byte: the controls, which take in every ASCII
/// whitespace but the space.
fn unseen_ascii(byte: u8) -> bool {
    byte.is_ascii_control()
}

impl<T: Canonical + ?Sized> Canonical for &T {
    fn canonical(&self, p: &mut Printer) {
        (**self).canonical(p);
    }

    fn canonical_with(&self, p: &mut Printer, arenas: &NodeArenas) {
        (**self).canonical_with(p, arenas);
    }
}

impl<T: Canonical + ?Sized> Canonical for Box<T> {
    fn canonical(&self, p: &mut Printer) {
        (**self).canonical(p);
    }

    fn canonical_with(&self, p: &mut Printer, arenas: &NodeArenas) {
        (**self).canonical_with(p, arenas);
    }
}

impl<T: Canonical + ?Sized> Canonical for Arc<T> {
    fn canonical(&self, p: &mut Printer) {
        (**self).canonical(p);
    }

    fn canonical_with(&self, p: &mut Printer, arenas: &NodeArenas) {
        (**self).canonical_with(p, arenas);
    }
}

/// What is stored: the value once set, `-` before.
impl<T: Canonical> Canonical for std::sync::OnceLock<T> {
    fn canonical(&self, p: &mut Printer) {
        self.get().canonical(p);
    }

    fn canonical_with(&self, p: &mut Printer, arenas: &NodeArenas) {
        self.get().canonical_with(p, arenas);
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

    fn canonical_with(&self, p: &mut Printer, arenas: &NodeArenas) {
        match self {
            Some(value) => value.canonical_with(p, arenas),
            None => p.atom("-"),
        }
    }
}

impl<T: Canonical> Canonical for [T] {
    fn canonical(&self, p: &mut Printer) {
        p.list(self);
    }

    fn canonical_with(&self, p: &mut Printer, arenas: &NodeArenas) {
        p.list_with(self, arenas);
    }
}

impl<T: Canonical> Canonical for Vec<T> {
    fn canonical(&self, p: &mut Printer) {
        p.list(self.as_slice());
    }

    fn canonical_with(&self, p: &mut Printer, arenas: &NodeArenas) {
        p.list_with(self.as_slice(), arenas);
    }
}

impl<A: Canonical, B: Canonical> Canonical for (A, B) {
    fn canonical(&self, p: &mut Printer) {
        let (a, b) = self;
        p.tuple("", &[a, b]);
    }
}

impl<A: Canonical, B: Canonical, C: Canonical> Canonical for (A, B, C) {
    fn canonical(&self, p: &mut Printer) {
        let (a, b, c) = self;
        p.tuple("", &[a, b, c]);
    }
}

/// A list printed as a set: for one whose order is not part of its value.
pub struct AsSet<'a, T>(pub &'a [T]);

impl<T: Canonical> Canonical for AsSet<'_, T> {
    fn canonical(&self, p: &mut Printer) {
        let AsSet(items) = self;
        p.set(items.iter());
    }
}

/// A map of lists printed with each list as a set: for one whose lists'
/// order is not part of its value.
pub struct MapOfSets<'a, K, V>(pub &'a rustc_hash::FxHashMap<K, Vec<V>>);

impl<K: Canonical, V: Canonical> Canonical for MapOfSets<'_, K, V> {
    fn canonical(&self, p: &mut Printer) {
        let MapOfSets(map) = self;
        let sets: Vec<(&K, AsSet<'_, V>)> = map
            .iter()
            .map(|(key, values)| (key, AsSet(values.as_slice())))
            .collect();
        p.map(sets.iter().map(|(key, values)| (*key, values)));
    }
}

/// A map prints sorted by its keys' prints: its order is the hasher's.
impl<K: Canonical, V: Canonical> Canonical for rustc_hash::FxHashMap<K, V> {
    fn canonical(&self, p: &mut Printer) {
        p.map(self.iter());
    }
}

/// A set prints sorted: its order is the hasher's.
impl<T: Canonical> Canonical for rustc_hash::FxHashSet<T> {
    fn canonical(&self, p: &mut Printer) {
        p.set(self.iter());
    }
}

impl<K: Canonical, V: Canonical, S> Canonical for hashbrown::HashMap<K, V, S> {
    fn canonical(&self, p: &mut Printer) {
        p.map(self.iter());
    }
}

impl<T: Canonical, S> Canonical for hashbrown::HashSet<T, S> {
    fn canonical(&self, p: &mut Printer) {
        p.set(self.iter());
    }
}

impl<K: Canonical, V: Canonical> Canonical for std::collections::BTreeMap<K, V> {
    fn canonical(&self, p: &mut Printer) {
        p.map(self.iter());
    }
}

impl<T: Canonical> Canonical for std::collections::BTreeSet<T> {
    fn canonical(&self, p: &mut Printer) {
        p.set(self.iter());
    }
}

/// `{:?}` prints the shortest text that reads back as the same bits, `-0.0`
/// and the infinities included; a NaN's payload only its bits show.
impl Canonical for f64 {
    fn canonical(&self, p: &mut Printer) {
        if self.is_nan() {
            p.atom(&format!("NaN(0x{:016x})", self.to_bits()));
        } else {
            p.atom(&format!("{self:?}"));
        }
    }
}

impl Canonical for char {
    fn canonical(&self, p: &mut Printer) {
        let mut buf = [0; 4];
        p.bytes(self.encode_utf8(&mut buf).as_bytes());
    }
}

/// A print of a struct from the list of its fields, as a node of them:
/// `canonical_struct!(Param { name, ty })`, or with `skip { cache }` after
/// it for fields that hold only what the others give. A tuple struct
/// names its fields by position: `canonical_struct!(Pair(left, right))`,
/// printed as a tuple.
#[macro_export]
macro_rules! canonical_struct {
    ($ty:ident { $($field:ident),* $(,)? } $(skip { $($skipped:ident),* $(,)? })?) => {
        impl $crate::canonical::Canonical for $ty {
            fn canonical(&self, p: &mut $crate::canonical::Printer) {
                let $ty { $($field,)* $($($skipped: _,)* ..)? } = self;
                p.node(
                    stringify!($ty),
                    &[$((stringify!($field), $field as &dyn $crate::canonical::Canonical)),*],
                );
            }
            #[allow(unused_variables)]
            fn canonical_with(
                &self,
                p: &mut $crate::canonical::Printer,
                arenas: &$crate::ast::NodeArenas,
            ) {
                let $ty { $($field,)* $($($skipped: _,)* ..)? } = self;
                p.node_with(
                    stringify!($ty),
                    &[$((stringify!($field), $field as &dyn $crate::canonical::Canonical)),*],
                    arenas,
                );
            }
        }
    };
    ($ty:ident ( $($field:ident),* $(,)? )) => {
        impl $crate::canonical::Canonical for $ty {
            fn canonical(&self, p: &mut $crate::canonical::Printer) {
                let $ty($($field),*) = self;
                p.tuple(
                    stringify!($ty),
                    &[$($field as &dyn $crate::canonical::Canonical),*],
                );
            }
            #[allow(unused_variables)]
            fn canonical_with(
                &self,
                p: &mut $crate::canonical::Printer,
                arenas: &$crate::ast::NodeArenas,
            ) {
                let $ty($($field),*) = self;
                p.tuple_with(
                    stringify!($ty),
                    &[$($field as &dyn $crate::canonical::Canonical),*],
                    arenas,
                );
            }
        }
    };
}

/// A print of an enum from the list of its variants, each with its fields:
/// `canonical_enum!(Kind { Unit, Pair(left, right), Named { a, b } })`. A
/// unit variant prints as its name, a tuple variant as a tuple and a
/// struct variant as a node.
#[macro_export]
macro_rules! canonical_enum {
    ($ty:ident { $($variant:ident $(( $($pos:ident),* $(,)? ))? $({ $($named:ident),* $(,)? })?),* $(,)? }) => {
        impl $crate::canonical::Canonical for $ty {
            fn canonical(&self, p: &mut $crate::canonical::Printer) {
                match self {
                    $(
                        $ty::$variant $(( $($pos),* ))? $({ $($named),* })? => {
                            $crate::canonical_enum!(
                                @print p, $variant $(( $($pos),* ))? $({ $($named),* })?
                            )
                        }
                    )*
                }
            }
            #[allow(unused_variables)]
            fn canonical_with(
                &self,
                p: &mut $crate::canonical::Printer,
                arenas: &$crate::ast::NodeArenas,
            ) {
                match self {
                    $(
                        $ty::$variant $(( $($pos),* ))? $({ $($named),* })? => {
                            $crate::canonical_enum!(
                                @print_with p, arenas, $variant $(( $($pos),* ))? $({ $($named),* })?
                            )
                        }
                    )*
                }
            }
        }
    };
    (@print $p:ident, $variant:ident ( $($pos:ident),* )) => {
        $p.tuple(
            stringify!($variant),
            &[$($pos as &dyn $crate::canonical::Canonical),*],
        )
    };
    (@print $p:ident, $variant:ident { $($named:ident),* }) => {
        $p.node(
            stringify!($variant),
            &[$((stringify!($named), $named as &dyn $crate::canonical::Canonical)),*],
        )
    };
    (@print $p:ident, $variant:ident) => {
        $p.atom(stringify!($variant))
    };
    (@print_with $p:ident, $arenas:ident, $variant:ident ( $($pos:ident),* )) => {
        $p.tuple_with(
            stringify!($variant),
            &[$($pos as &dyn $crate::canonical::Canonical),*],
            $arenas,
        )
    };
    (@print_with $p:ident, $arenas:ident, $variant:ident { $($named:ident),* }) => {
        $p.node_with(
            stringify!($variant),
            &[$((stringify!($named), $named as &dyn $crate::canonical::Canonical)),*],
            $arenas,
        )
    };
    (@print_with $p:ident, $arenas:ident, $variant:ident) => {
        $p.atom(stringify!($variant))
    };
}

impl Canonical for () {
    fn canonical(&self, p: &mut Printer) {
        p.atom("()");
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

/// A map by name prints sorted by name: its order is the table's.
impl<V: Canonical> Canonical for crate::name::NameMap<V> {
    fn canonical(&self, p: &mut Printer) {
        p.map(self.iter());
    }
}

/// In the order the names were first inserted, which is the map's own.
impl<V: Canonical> Canonical for crate::name::OrderedNameMap<V> {
    fn canonical(&self, p: &mut Printer) {
        p.list(self.iter().collect::<Vec<_>>().iter());
    }
}

impl<V: Canonical> Canonical for crate::name::FoldedMap<V> {
    fn canonical(&self, p: &mut Printer) {
        p.map(self.iter());
    }
}

impl Canonical for crate::name::NameSet {
    fn canonical(&self, p: &mut Printer) {
        p.set(self.iter());
    }
}

impl Canonical for crate::name::FoldedSet {
    fn canonical(&self, p: &mut Printer) {
        p.set(self.iter());
    }
}

/// A path's bytes, quoted.
impl Canonical for std::path::Path {
    fn canonical(&self, p: &mut Printer) {
        p.bytes(self.as_os_str().as_encoded_bytes());
    }
}

impl Canonical for std::path::PathBuf {
    fn canonical(&self, p: &mut Printer) {
        self.as_path().canonical(p);
    }
}

impl Canonical for crate::references::RefKey {
    fn canonical(&self, p: &mut Printer) {
        use crate::references::RefKey;
        let (kind, name) = match self {
            RefKey::Class(name) => ("Class", name),
            RefKey::Function(name) => ("Function", name),
            RefKey::Constant(name, _) => ("Constant", name),
            RefKey::GlobalVar(name) => ("GlobalVar", name),
        };
        p.tuple(kind, &[name]);
    }
}

crate::canonical_struct!(DocSite {
    doc,
    pos,
    outside,
    scope,
});

crate::canonical_struct!(DocFrame { doc, start, end });

impl Canonical for Span {
    fn canonical(&self, p: &mut Printer) {
        let Span { start, end } = self;
        p.atom(&format!("{start}..{end}"));
    }
}

/// Serialises a hash map with its entries in key order, so that two maps of
/// the same entries encode to the same bytes whatever order each was filled
/// in. For `#[serde(serialize_with = ...)]`.
pub fn serialize_in_key_order<K, V, S>(
    map: &rustc_hash::FxHashMap<K, V>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    K: Ord + serde::Serialize,
    V: serde::Serialize,
    S: serde::Serializer,
{
    let mut entries: Vec<(&K, &V)> = map.iter().collect();
    entries.sort_unstable_by(|a, b| a.0.cmp(b.0));
    serializer.collect_map(entries)
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

    /// A character at a time, as the escapes are specified.
    fn quoted_by_character(bytes: &[u8]) -> String {
        let mut out = String::from("'");
        for chunk in bytes.utf8_chunks() {
            for ch in chunk.valid().chars() {
                match ch {
                    '\\' => out.push_str("\\\\"),
                    '\'' => out.push_str("\\'"),
                    ch if unseen(ch) => {
                        for byte in ch.to_string().bytes() {
                            out.push_str(&format!("\\x{byte:02x}"));
                        }
                    }
                    ch => out.push(ch),
                }
            }
            for byte in chunk.invalid() {
                out.push_str(&format!("\\x{byte:02x}"));
            }
        }
        out.push('\'');
        out
    }

    fn name_by_character(name: &str) -> String {
        let mut out = String::new();
        for ch in name.chars() {
            if unseen(ch) || ch == ' ' {
                for byte in ch.to_string().bytes() {
                    out.push_str(&format!("\\x{{{byte:02x}}}"));
                }
            } else {
                out.push(ch);
            }
        }
        out
    }

    #[test]
    fn runs_of_plain_bytes_print_as_their_characters_would_one_at_a_time() {
        let mut texts: Vec<Vec<u8>> = (0..=255u8)
            .map(|byte| [b"ab".as_slice(), &[byte], b"cd"].concat())
            .collect();
        texts.push((0..=255u8).collect());
        for sample in [
            "",
            " ",
            "plain",
            "\u{85}",
            "\u{a0}x",
            "x\u{2028}",
            "\u{3000}\u{200b}",
            "é日本",
            "it's \\ a\tb\nc",
            "\u{7f}\u{9f}\u{1680} \u{feff}",
        ] {
            texts.push(sample.as_bytes().to_vec());
            texts.push(format!("{sample}{sample}").into_bytes());
        }
        texts.push(b"\xe2\x80 \xff\xc3\xa9\xc3".to_vec());
        for text in &texts {
            let mut out = String::new();
            quote_bytes(text, &mut out);
            assert_eq!(out, quoted_by_character(text), "{text:?}");
            let name = String::from_utf8_lossy(text);
            let mut out = String::new();
            write_name(&name, &mut out);
            assert_eq!(out, name_by_character(&name), "{name:?}");
        }
    }
}
