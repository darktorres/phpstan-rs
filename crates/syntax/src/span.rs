//! Where things are in a file: a line index from byte offsets to lines and
//! columns, which a file's diagnostics and editor positions are made with.

use crate::token::Position;

/// The lines of a file, and the characters on them, found from its bytes
/// once: a position is made from a byte offset when someone asks for it,
/// so the lexer and parser carry offsets.
///
/// A column counts characters from the start of the line, and a run of
/// invalid bytes a lossy reading replaces counts as one.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LineIndex {
    /// The offset each line begins at; the first is 0.
    line_starts: Box<[u32]>,
    /// The length of the source.
    len: u32,
    /// For a source that is not all ASCII: after each multi-byte character
    /// (or run of invalid bytes), the offset it ends at and the bytes
    /// beyond one for each character up to there.
    surplus: Box<[(u32, u32)]>,
}

impl LineIndex {
    pub fn new(src: &[u8]) -> LineIndex {
        // A line of PHP is about thirty bytes.
        let mut line_starts: Vec<u32> = Vec::with_capacity(src.len() / 32 + 1);
        line_starts.push(0);
        line_starts.extend(memchr::memchr_iter(b'\n', src).map(|i| i as u32 + 1));
        let mut surplus = Vec::new();
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

    /// The line and column of `offset`, which is the end of the source when
    /// it is past it.
    pub fn line_col(&self, offset: u32) -> Position {
        let offset = offset.min(self.len);
        let index = self.line_index(offset);
        let start = self.line_starts[index];
        let bytes = offset - start;
        let beyond = self.surplus_to(offset) - self.surplus_to(start);
        Position {
            line: index as u32 + 1,
            col: 1 + bytes - beyond,
        }
    }

    /// The offset of `at`, or none when its line is not in the source or
    /// its column is past the end of the line.
    pub fn offset(&self, at: Position) -> Option<u32> {
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

    fn at(line: u32, col: u32) -> Position {
        Position { line, col }
    }

    #[test]
    fn an_ascii_source_has_a_column_for_each_byte() {
        let index = LineIndex::new(b"<?php\n$a = 1;\n\n$b;");
        assert_eq!(index.lines(), 4);
        assert_eq!(index.line_col(0), at(1, 1));
        assert_eq!(index.line_col(5), at(1, 6));
        assert_eq!(index.line_col(6), at(2, 1));
        assert_eq!(index.line_col(13), at(2, 8));
        assert_eq!(index.line_col(15), at(3, 1));
        assert_eq!(index.line_col(16), at(4, 1));
        assert_eq!(index.line_col(99), at(4, 4));
        assert_eq!(index.line(14), 2);
        assert_eq!(index.line_start(4), Some(16));
        assert_eq!(index.line_end(2), Some(13));
        assert_eq!(index.line_end(4), Some(19));
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
        assert_eq!(index.offset(at(1, 7)), Some(11));
        assert_eq!(index.offset(at(1, 8)), None);
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
}
