//! Where things are in a file: a span is a pair of byte offsets, and a
//! line index turns an offset into the line and column a diagnostic or an
//! editor shows.

/// A stretch of a file, from the byte offset it starts at to the one it
/// ends at, the byte after its last.
///
/// `Hash`: `Locals`'s own declaration-provenance tracking uses a `Span`
/// as a `HashSet` element, one entry per candidate declaration site for
/// a variable.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
#[cfg_attr(feature = "fuzzing", derive(arbitrary::Arbitrary))]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub fn new(start: u32, end: u32) -> Span {
        Span { start, end }
    }

    /// The bytes it covers.
    pub fn len(self) -> u32 {
        self.end.saturating_sub(self.start)
    }

    pub fn is_empty(self) -> bool {
        self.end <= self.start
    }

    /// Whether the byte at `offset` is in it.
    pub fn contains(self, offset: u32) -> bool {
        self.start <= offset && offset < self.end
    }

    /// From where this starts to where `other` ends.
    pub fn to(self, other: Span) -> Span {
        Span {
            start: self.start,
            end: other.end,
        }
    }

    /// The bytes of `source` it covers, or none when it is past the end.
    pub fn slice(self, source: &[u8]) -> Option<&[u8]> {
        source.get(self.start as usize..self.end as usize)
    }
}

/// A point in a file as a person counts it, 1-indexed (matching editor
/// convention). `col` counts UTF-8 characters, not raw bytes: PHP source
/// has multi-byte characters inside code, not only in comments (string
/// literals with em-dashes, curly quotes, accents), and a byte column
/// would overshoot the character position on every such line.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Default,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
)]
#[cfg_attr(feature = "fuzzing", derive(arbitrary::Arbitrary))]
pub struct LineCol {
    pub line: u32,
    pub col: u32,
}

/// A range in a file as a person counts it: what an editor or an output
/// format wants of a diagnostic, a start and an end.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
#[cfg_attr(feature = "fuzzing", derive(arbitrary::Arbitrary))]
pub struct Range {
    pub start: LineCol,
    pub end: LineCol,
}

/// What a column of a line counts, as an editor protocol agrees it: bytes
/// of UTF-8, or code units of UTF-16.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Unit {
    Utf8,
    #[default]
    Utf16,
}

/// How many bytes the line break at the start of `bytes` takes: two for a
/// carriage return and line feed, one for either alone, none when `bytes`
/// does not start with one. PHP, editors and the language server protocol
/// agree that a line ends at any of the three.
pub fn line_break_len(bytes: &[u8]) -> usize {
    match bytes {
        [b'\r', b'\n', ..] => 2,
        [b'\r' | b'\n', ..] => 1,
        _ => 0,
    }
}

/// The lines of `text`, each without its line break. As `str::lines`, a
/// last line break does not start another line, and a carriage return alone
/// ends a line too.
pub fn lines(text: &str) -> impl Iterator<Item = &str> {
    let mut rest = text;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let bytes = rest.as_bytes();
        let Some(end) = memchr::memchr2(b'\n', b'\r', bytes) else {
            return Some(std::mem::take(&mut rest));
        };
        let (line, after) = rest.split_at(end);
        rest = &after[line_break_len(after.as_bytes())..];
        Some(line)
    })
}

/// How many line breaks `text` holds.
pub fn count_line_breaks(text: &str) -> usize {
    let bytes = text.as_bytes();
    let mut count = 0;
    let mut at = 0;
    while let Some(found) = memchr::memchr2(b'\n', b'\r', &bytes[at..]) {
        at += found;
        at += line_break_len(&bytes[at..]);
        count += 1;
    }
    count
}

/// The line break a file writes between its lines.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum LineBreak {
    /// A line feed, which Unix writes.
    #[default]
    Lf,
    /// A carriage return and line feed, which Windows writes.
    CrLf,
    /// A carriage return alone, which Classic Mac OS wrote.
    Cr,
}

impl LineBreak {
    pub fn as_str(self) -> &'static str {
        match self {
            LineBreak::Lf => "\n",
            LineBreak::CrLf => "\r\n",
            LineBreak::Cr => "\r",
        }
    }
}

/// The lines of a file, and the characters on them, found from its bytes
/// once: a position is made from a byte offset when someone asks for it,
/// so the lexer and parser carry offsets.
///
/// A line ends at a line feed, a carriage return and line feed, or a
/// carriage return alone. A column counts characters from the start of the
/// line, and a run of invalid bytes a lossy reading replaces counts as one.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LineIndex {
    /// The offset each line begins at; the first is 0.
    line_starts: Box<[u32]>,
    /// A bit for each line, set when a carriage return and line feed ends
    /// it; empty for a source with none.
    crlf: Box<[u64]>,
    /// The first line break of the source.
    first_break: LineBreak,
    /// The length of the source.
    len: u32,
    /// For a source that is not all ASCII: after each multi-byte character
    /// (or run of invalid bytes), the offset it ends at and the bytes
    /// beyond one for each character up to there.
    surplus: Box<[(u32, u32)]>,
    /// The offset each four-byte character starts at, which UTF-16 spends
    /// two code units on.
    astral: Box<[u32]>,
}

impl Default for LineIndex {
    /// The index of an empty source.
    fn default() -> Self {
        LineIndex::new(b"")
    }
}

/// Adds the start of each line after the first to `line_starts`, and sets
/// the bit of each line a carriage return and line feed ends in `crlf`, for
/// a source that holds a carriage return. Returns the first line break the
/// source writes.
fn scan_breaks(src: &[u8], line_starts: &mut Vec<u32>, crlf: &mut Vec<u64>) -> LineBreak {
    let mut first_break = None;
    let mut after_carriage_return = false;
    for at in memchr::memchr2_iter(b'\n', b'\r', src) {
        let kind = if src[at] == b'\n' {
            if std::mem::take(&mut after_carriage_return) {
                LineBreak::CrLf
            } else {
                LineBreak::Lf
            }
        } else if src.get(at + 1) == Some(&b'\n') {
            // The line feed after it ends the line.
            after_carriage_return = true;
            continue;
        } else {
            LineBreak::Cr
        };
        first_break.get_or_insert(kind);
        if kind == LineBreak::CrLf {
            let line = line_starts.len() - 1;
            if crlf.len() <= line / 64 {
                crlf.resize(line / 64 + 1, 0);
            }
            crlf[line / 64] |= 1 << (line % 64);
        }
        line_starts.push(at as u32 + 1);
    }
    first_break.unwrap_or_default()
}

impl LineIndex {
    pub fn new(src: &[u8]) -> LineIndex {
        // A line of PHP is about thirty bytes.
        let mut line_starts: Vec<u32> = Vec::with_capacity(src.len() / 32 + 1);
        line_starts.push(0);
        let mut crlf: Vec<u64> = Vec::new();
        let first_break = if memchr::memchr(b'\r', src).is_none() {
            line_starts.extend(memchr::memchr_iter(b'\n', src).map(|at| at as u32 + 1));
            LineBreak::Lf
        } else {
            scan_breaks(src, &mut line_starts, &mut crlf)
        };
        let mut surplus = Vec::new();
        let mut astral = Vec::new();
        if !src.is_ascii() {
            let mut base = 0usize;
            let mut total = 0u32;
            for chunk in src.utf8_chunks() {
                let valid = chunk.valid().as_bytes();
                let mut from = 0;
                while let Some(found) = valid[from..].iter().position(|&b| b >= 0x80) {
                    let at = from + found;
                    let width = match valid[at] {
                        0xF0.. => 4,
                        0xE0.. => 3,
                        _ => 2,
                    };
                    if width == 4 {
                        astral.push((base + at) as u32);
                    }
                    from = at + width;
                    total += width as u32 - 1;
                    surplus.push(((base + from) as u32, total));
                }
                let invalid = chunk.invalid().len();
                if invalid > 1 {
                    total += invalid as u32 - 1;
                    surplus.push(((base + valid.len() + invalid) as u32, total));
                }
                base += valid.len() + invalid;
            }
        }
        LineIndex {
            line_starts: line_starts.into_boxed_slice(),
            crlf: crlf.into_boxed_slice(),
            first_break,
            len: src.len() as u32,
            surplus: surplus.into_boxed_slice(),
            astral: astral.into_boxed_slice(),
        }
    }

    /// The number of lines: one more than the line breaks.
    pub fn lines(&self) -> u32 {
        self.line_starts.len() as u32
    }

    /// The line break the source writes first, which a line that is added
    /// to it should end with.
    pub fn line_break(&self) -> LineBreak {
        self.first_break
    }

    /// Whether a carriage return and line feed ends the 0-based line.
    fn ends_with_crlf(&self, line: usize) -> bool {
        self.crlf
            .get(line / 64)
            .is_some_and(|word| word >> (line % 64) & 1 == 1)
    }

    /// The 0-based index of the line `offset` is on.
    fn line_index(&self, offset: u32) -> usize {
        self.line_starts
            .partition_point(|&start| start <= offset)
            .saturating_sub(1)
    }

    /// The bytes beyond one for each character in the first `offset` bytes.
    fn surplus_to(&self, offset: u32) -> u32 {
        match self.surplus.partition_point(|&(end, _)| end <= offset) {
            0 => 0,
            n => self.surplus[n - 1].1,
        }
    }

    /// The 1-based line `offset` is on, without its column.
    pub fn line(&self, offset: u32) -> u32 {
        self.line_index(offset.min(self.len)) as u32 + 1
    }

    /// The offset the 1-based `line` starts at.
    pub fn line_start(&self, line: u32) -> Option<u32> {
        let index = (line as usize).checked_sub(1)?;
        self.line_starts.get(index).copied()
    }

    /// The offset the 1-based `line` ends at, before its line break.
    pub fn line_end(&self, line: u32) -> Option<u32> {
        let index = (line as usize).checked_sub(1)?;
        self.line_starts.get(index)?;
        Some(match self.line_starts.get(index + 1) {
            Some(&next) => next - 1 - u32::from(self.ends_with_crlf(index)),
            None => self.len,
        })
    }

    /// The lines and columns a span covers.
    pub fn range(&self, span: Span) -> Range {
        Range {
            start: self.line_col(span.start),
            end: self.line_col(span.end),
        }
    }

    /// The line and column of `offset`, which is the end of the source when
    /// it is past it.
    pub fn line_col(&self, offset: u32) -> LineCol {
        let offset = offset.min(self.len);
        let index = self.line_index(offset);
        let start = self.line_starts[index];
        let bytes = offset - start;
        let beyond = self.surplus_to(offset) - self.surplus_to(start);
        LineCol {
            line: index as u32 + 1,
            col: 1 + bytes - beyond,
        }
    }

    /// The 0-based column of `offset` on its line, counted in `unit`.
    pub fn column(&self, offset: u32, unit: Unit) -> u32 {
        let offset = offset.min(self.len);
        let start = self.line_starts[self.line_index(offset)];
        match unit {
            Unit::Utf8 => offset - start,
            Unit::Utf16 => {
                let characters = self.line_col(offset).col - 1;
                characters + self.astral_between(start, offset)
            }
        }
    }

    /// How many four-byte characters start in `from..to`.
    fn astral_between(&self, from: u32, to: u32) -> u32 {
        let below = |offset: u32| self.astral.partition_point(|&at| at < offset);
        (below(to) - below(from)) as u32
    }

    /// The offset of the 0-based `line` and the column `column` counts in
    /// `unit`, or none when the line is not in the source or the column is
    /// past the end of the line. A UTF-16 column inside a character that
    /// takes two units is the character's start.
    pub fn offset_in(&self, line: u32, column: u32, unit: Unit) -> Option<u32> {
        let start = self.line_start(line.checked_add(1)?)?;
        let end = self.line_end(line + 1)?;
        match unit {
            Unit::Utf8 => {
                let offset = start.checked_add(column)?;
                (offset <= end).then_some(offset)
            }
            Unit::Utf16 => {
                let first = self.astral.partition_point(|&at| at < start);
                let mut extra = 0;
                for (k, &at) in self.astral[first..].iter().enumerate() {
                    if at >= end {
                        break;
                    }
                    // Where the character starts, in UTF-16 units.
                    let units = self.line_col(at).col - 1 + k as u32;
                    if column >= units + 2 {
                        extra = k as u32 + 1;
                    } else {
                        if column == units + 1 {
                            extra = k as u32 + 1;
                        }
                        break;
                    }
                }
                self.offset(LineCol {
                    line: line + 1,
                    col: column.checked_sub(extra)? + 1,
                })
            }
        }
    }

    /// `offset_in`, for a position this source may not have: a line past the
    /// last is the last, and a column past the end of its line is the end.
    pub fn offset_in_clamped(&self, line: u32, column: u32, unit: Unit) -> u32 {
        let line = line.min(self.lines() - 1);
        let end = self.line_end(line + 1).unwrap_or(self.len);
        self.offset_in(line, column, unit).unwrap_or(end)
    }

    /// The offset of `at`, or none when its line is not in the source or
    /// its column is past the end of the line.
    pub fn offset(&self, at: LineCol) -> Option<u32> {
        let start = self.line_start(at.line)?;
        let end = self.line_end(at.line)?;
        let want = at.col.checked_sub(1)?;
        let before = self.surplus_to(start);
        let mut beyond = 0;
        let first = self.surplus.partition_point(|&(end, _)| end <= start);
        for &(stop, total) in &self.surplus[first..] {
            // The characters up to the end of this one.
            if stop - start - (total - before) > want {
                break;
            }
            beyond = total - before;
        }
        let offset = start + want + beyond;
        (offset <= end).then_some(offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(line: u32, col: u32) -> LineCol {
        LineCol { line, col }
    }

    #[test]
    fn an_ascii_source_has_a_column_for_each_byte() {
        let index = LineIndex::new(b"<?php\n$a = 1;\n\n$b;");
        assert_eq!(index.lines(), 4);
        assert_eq!(index.line_col(0), at(1, 1));
        assert_eq!(index.line_col(5), at(1, 6));
        assert_eq!(index.line_col(6), at(2, 1));
        assert_eq!(index.line_col(13), at(2, 8));
        assert_eq!(index.line_col(14), at(3, 1));
        assert_eq!(index.line_col(15), at(4, 1));
        assert_eq!(index.line_col(99), at(4, 4));
        assert_eq!(index.line(13), 2);
        assert_eq!(index.line(14), 3);
        assert_eq!(index.line_start(4), Some(15));
        assert_eq!(index.line_end(2), Some(13));
        assert_eq!(index.line_end(4), Some(18));
        assert_eq!(index.line_start(5), None);
        assert_eq!(index.line_start(0), None);
    }

    /// `source`, which writes each line break as `\n`, written with each of
    /// the three line breaks.
    fn written_three_ways(source: &str) -> [(LineBreak, String); 3] {
        [LineBreak::Lf, LineBreak::CrLf, LineBreak::Cr]
            .map(|line_break| (line_break, source.replace('\n', line_break.as_str())))
    }

    #[test]
    fn a_line_ends_at_any_of_the_three_line_breaks() {
        for (line_break, source) in written_three_ways("<?php\n$a = 1;\n\n$b;") {
            let index = LineIndex::new(source.as_bytes());
            let width = line_break.as_str().len() as u32;
            assert_eq!(index.lines(), 4, "{line_break:?}");
            assert_eq!(index.line_start(2), Some(5 + width), "{line_break:?}");
            assert_eq!(index.line_end(2), Some(5 + width + 7), "{line_break:?}");
            assert_eq!(index.line_end(1), Some(5), "{line_break:?}");
            assert_eq!(index.line_start(4), Some(5 + width + 7 + 2 * width));
            assert_eq!(index.line_end(4), Some(source.len() as u32));
            // The break belongs to the line it ends: the first byte of a
            // `\r\n` is on the line before the second.
            assert_eq!(index.line_col(5), at(1, 6), "{line_break:?}");
            assert_eq!(index.line_col(5 + width), at(2, 1), "{line_break:?}");
            assert_eq!(index.line_col(source.len() as u32), at(4, 4));
            assert_eq!(index.line_break(), line_break);
        }
        let crlf = LineIndex::new(b"ab\r\ncd");
        assert_eq!(crlf.line_col(2), at(1, 3));
        assert_eq!(crlf.line_col(3), at(1, 4));
        assert_eq!(crlf.line_col(4), at(2, 1));
    }

    #[test]
    fn a_carriage_return_before_a_line_feed_is_one_line_break() {
        let index = LineIndex::new(b"a\r\r\n\n\r\rb\r");
        let starts: Vec<u32> = (1..=index.lines())
            .map(|line| index.line_start(line).unwrap())
            .collect();
        // `a\r`, `\r\n`, `\n`, `\r`, `\r`, `b\r`, and the empty last line.
        assert_eq!(starts, [0, 2, 4, 5, 6, 7, 9]);
        assert_eq!(index.line_end(2), Some(2));
        assert_eq!(index.line_end(1), Some(1));
        assert_eq!(index.line_end(6), Some(8));
        assert_eq!(index.line_end(7), Some(9));
        assert_eq!(index.line_break(), LineBreak::Cr);
    }

    #[test]
    fn a_source_has_the_line_break_it_writes_first() {
        assert_eq!(LineIndex::new(b"").line_break(), LineBreak::Lf);
        assert_eq!(LineIndex::new(b"no breaks").line_break(), LineBreak::Lf);
        assert_eq!(LineIndex::new(b"a\r\nb\nc").line_break(), LineBreak::CrLf);
        assert_eq!(LineIndex::new(b"a\nb\r\nc").line_break(), LineBreak::Lf);
        assert_eq!(LineIndex::new(b"a\rb\n").line_break(), LineBreak::Cr);
        assert_eq!(LineBreak::CrLf.as_str(), "\r\n");
    }

    #[test]
    fn a_line_end_leaves_out_each_lines_own_break_in_a_mixed_source() {
        let source = "a\nb\r\nc\rd\r\n";
        let index = LineIndex::new(source.as_bytes());
        let ends: Vec<u32> = (1..=index.lines())
            .map(|line| index.line_end(line).unwrap())
            .collect();
        assert_eq!(ends, [1, 3, 6, 8, 10]);
        for line in 1..=index.lines() {
            let (start, end) = (
                index.line_start(line).unwrap(),
                index.line_end(line).unwrap(),
            );
            assert!(
                !source[start as usize..end as usize].contains(['\r', '\n']),
                "line {line}"
            );
        }
    }

    #[test]
    fn a_position_is_found_on_lines_that_end_in_any_break() {
        for (line_break, source) in written_three_ways("ab\ncd\n\u{e9}f") {
            let index = LineIndex::new(source.as_bytes());
            assert_eq!(index.offset_in(1, 0, Unit::Utf16), index.line_start(2));
            // The end of a line is before its break, and no column is past it.
            let end = index.line_end(1).unwrap();
            assert_eq!(
                index.offset_in(0, 2, Unit::Utf16),
                Some(end),
                "{line_break:?}"
            );
            assert_eq!(index.offset_in(0, 3, Unit::Utf16), None, "{line_break:?}");
            assert_eq!(index.offset_in(0, 3, Unit::Utf8), None, "{line_break:?}");
            assert_eq!(index.offset_in_clamped(0, 99, Unit::Utf16), end);
            assert_eq!(index.offset(at(1, 3)), Some(end), "{line_break:?}");
            assert_eq!(index.offset(at(1, 4)), None, "{line_break:?}");
            // A column on the last line counts the characters before it.
            let start = index.line_start(3).unwrap();
            assert_eq!(index.offset_in(2, 2, Unit::Utf16), Some(start + 3));
            assert_eq!(index.column(start + 3, Unit::Utf16), 2);
        }
    }

    #[test]
    fn lines_are_split_at_any_line_break_and_a_last_break_starts_no_line() {
        let split = |text| -> Vec<String> { lines(text).map(str::to_string).collect() };
        assert_eq!(split(""), Vec::<String>::new());
        assert_eq!(split("a"), ["a"]);
        assert_eq!(split("a\n"), ["a"]);
        assert_eq!(split("a\r\n"), ["a"]);
        assert_eq!(split("a\r"), ["a"]);
        assert_eq!(split("a\nb\r\nc\rd"), ["a", "b", "c", "d"]);
        assert_eq!(split("\n\r\n\r"), ["", "", ""]);
        assert_eq!(split("a\r\r\nb"), ["a", "", "b"]);
        for text in ["", "a", "a\n", "a\nb\n\nc", "\n\nx\r\n", "a\r\nb\r\n"] {
            assert_eq!(split(text), text.lines().collect::<Vec<_>>(), "{text:?}");
        }
    }

    #[test]
    fn a_source_without_a_carriage_return_is_scanned_as_the_general_scan_does() {
        for text in ["", "a", "\n", "a\nb", "a\n\nb\n", "\u{e9}\n\u{1f600}\nx"] {
            let mut line_starts = vec![0];
            let mut crlf = Vec::new();
            let first_break = scan_breaks(text.as_bytes(), &mut line_starts, &mut crlf);
            let index = LineIndex::new(text.as_bytes());
            assert_eq!(&*index.line_starts, &line_starts[..], "{text:?}");
            assert!(index.crlf.is_empty() && crlf.is_empty(), "{text:?}");
            assert_eq!(index.first_break, first_break, "{text:?}");
        }
    }

    #[test]
    fn line_breaks_are_counted_as_the_index_counts_them() {
        for text in ["", "a", "a\n", "a\r\nb", "a\rb", "\r\r\n\n\r", "x\r\n\r\ny"] {
            assert_eq!(
                count_line_breaks(text) + 1,
                LineIndex::new(text.as_bytes()).lines() as usize,
                "{text:?}"
            );
        }
    }

    #[test]
    fn the_length_of_a_line_break_is_what_starts_the_bytes() {
        assert_eq!(line_break_len(b""), 0);
        assert_eq!(line_break_len(b"a\n"), 0);
        assert_eq!(line_break_len(b"\n"), 1);
        assert_eq!(line_break_len(b"\r"), 1);
        assert_eq!(line_break_len(b"\r\n"), 2);
        assert_eq!(line_break_len(b"\r\r\n"), 1);
        assert_eq!(line_break_len(b"\n\r"), 1);
    }

    #[test]
    fn a_column_counts_characters_not_bytes() {
        let source = "a\u{e9}\u{4e2d}\u{10348}b\n\u{e9}x";
        let index = LineIndex::new(source.as_bytes());
        let mut expected = Vec::new();
        let (mut line, mut col) = (1, 1);
        for (offset, c) in source.char_indices() {
            expected.push((offset as u32, at(line, col)));
            if c == '\n' {
                (line, col) = (line + 1, 1);
            } else {
                col += 1;
            }
        }
        for (offset, position) in expected {
            assert_eq!(index.line_col(offset), position, "{offset}");
            assert_eq!(index.offset(position), Some(offset), "{position:?}");
        }
        assert_eq!(index.offset(at(1, 6)), Some(11));
        assert_eq!(index.offset(at(1, 7)), None);
        assert_eq!(index.offset(at(3, 1)), None);
    }

    #[test]
    fn a_run_of_invalid_bytes_is_one_column() {
        let source = b"a\xff\xfeb\xe2\x82c";
        let index = LineIndex::new(source);
        let lossy = String::from_utf8_lossy(source);
        assert_eq!(lossy.chars().count(), 6);
        assert_eq!(index.line_col(0), at(1, 1));
        assert_eq!(index.line_col(1), at(1, 2));
        assert_eq!(index.line_col(3), at(1, 4));
        assert_eq!(index.line_col(4), at(1, 5));
        assert_eq!(index.line_col(6), at(1, 6));
        assert_eq!(index.line_col(7), at(1, 7));
    }

    #[test]
    fn a_column_counts_bytes_or_utf16_units() {
        // `é` is two bytes and one unit, `日` three and one, `😀` four and two.
        let source = "a\u{e9}\u{65e5}\u{1f600}b\nc\u{1f600}d".as_bytes();
        let index = LineIndex::new(source);
        let offsets: Vec<(u32, u32, u32)> = vec![
            // (offset, utf8 column, utf16 column)
            (0, 0, 0),
            (1, 1, 1),
            (3, 3, 2),
            (6, 6, 3),
            (10, 10, 5),
            (11, 11, 6),
        ];
        for (offset, utf8, utf16) in offsets {
            assert_eq!(index.column(offset, Unit::Utf8), utf8, "{offset}");
            assert_eq!(index.column(offset, Unit::Utf16), utf16, "{offset}");
            assert_eq!(index.offset_in(0, utf8, Unit::Utf8), Some(offset));
            assert_eq!(index.offset_in(0, utf16, Unit::Utf16), Some(offset));
        }
        // The second line starts after the line feed at 11.
        assert_eq!(index.column(17, Unit::Utf16), 3);
        assert_eq!(index.offset_in(1, 3, Unit::Utf16), Some(17));
        assert_eq!(index.column(17, Unit::Utf8), 5);
        // Inside a surrogate pair is the start of the character.
        assert_eq!(index.offset_in(0, 4, Unit::Utf16), Some(6));
        assert_eq!(index.offset_in(0, 7, Unit::Utf16), None);
        assert_eq!(index.offset_in(2, 0, Unit::Utf16), None);
    }
}
