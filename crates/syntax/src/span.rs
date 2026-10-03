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

/// The lines of a file, and the characters on them, found from its bytes
/// once: a position is made from a byte offset when someone asks for it,
/// so the lexer and parser carry offsets.
///
/// A column counts characters from the start of the line, and a run of
/// invalid bytes a lossy reading replaces counts as one.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LineIndex {
    /// The offset each line begins at; the first is 0.
    line_starts: Box<[u32]>,
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

impl LineIndex {
    pub fn new(src: &[u8]) -> LineIndex {
        // A line of PHP is about thirty bytes.
        let mut line_starts: Vec<u32> = Vec::with_capacity(src.len() / 32 + 1);
        line_starts.push(0);
        line_starts.extend(memchr::memchr_iter(b'\n', src).map(|i| i as u32 + 1));
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
            len: src.len() as u32,
            surplus: surplus.into_boxed_slice(),
            astral: astral.into_boxed_slice(),
        }
    }

    /// The number of lines: one more than the line feeds.
    pub fn lines(&self) -> u32 {
        self.line_starts.len() as u32
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

    /// The offset the 1-based `line` ends at, before its line feed.
    pub fn line_end(&self, line: u32) -> Option<u32> {
        let index = (line as usize).checked_sub(1)?;
        self.line_starts.get(index)?;
        Some(match self.line_starts.get(index + 1) {
            Some(&next) => next - 1,
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
