//! The stub cache's file: named sections in one byte buffer, which a
//! reader keeps whole and hands out ranges of, so what is stored is read in
//! place rather than decoded into a value of its own.
//!
//! A file is a header, the body (what the sections and the tables' records
//! point into, by offset from the start of the file) and a directory of the
//! sections by name. The header carries a checksum of everything after it.

use std::ops::Range;
use std::sync::Arc;

use rustc_hash::FxHashMap;

const MAGIC: [u8; 8] = *b"PRSSTUBS";

/// Bumped with any change to the layout, so a file of another layout is
/// rebuilt instead of read.
const VERSION: u32 = 3;

/// Magic, version, the body's length, the directory's length, the
/// checksum.
pub(super) const HEADER_LEN: usize = 8 + 4 + 4 + 8 + 8 + 8;

/// What the file's `checksum` covers is the bytes after the header: the
/// body, then the directory. A CRC, which the processor computes a block at
/// a time: it only has to tell a damaged file from a whole one.
pub(super) fn checksum(bytes: &[u8]) -> u64 {
    u64::from(crc32fast::hash(bytes))
}

/// Where bytes sit in the file: the offset from its start and the length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Span {
    pub(super) offset: u32,
    pub(super) len: u32,
}

impl Span {
    pub(super) fn range(self) -> Range<usize> {
        self.offset as usize..self.offset as usize + self.len as usize
    }
}

/// Builds a file.
pub(crate) struct Writer {
    body: Vec<u8>,
    sections: Vec<(String, Span)>,
    // The bytes already written, to write equal bytes once: names by their
    // text, entries by a hash of them.
    names: FxHashMap<String, Span>,
    entries: FxHashMap<u64, Vec<Span>>,
}

impl Writer {
    pub(super) fn new() -> Writer {
        Writer {
            body: Vec::new(),
            sections: Vec::new(),
            names: FxHashMap::default(),
            entries: FxHashMap::default(),
        }
    }

    #[cfg(test)]
    pub(super) fn body_len(&self) -> usize {
        self.body.len()
    }

    fn append(&mut self, bytes: &[u8]) -> Result<Span, String> {
        let offset = u32::try_from(HEADER_LEN + self.body.len())
            .map_err(|_| "the stored stubs outgrew four gigabytes".to_string())?;
        let len = u32::try_from(bytes.len())
            .map_err(|_| "a stored entry outgrew four gigabytes".to_string())?;
        self.body.extend_from_slice(bytes);
        // The end of what was written must still be an offset.
        u32::try_from(HEADER_LEN + self.body.len())
            .map_err(|_| "the stored stubs outgrew four gigabytes".to_string())?;
        Ok(Span { offset, len })
    }

    /// `text` written once, however many tables name it.
    pub(super) fn name(&mut self, text: &str) -> Result<Span, String> {
        if let Some(span) = self.names.get(text) {
            return Ok(*span);
        }
        let span = self.append(text.as_bytes())?;
        self.names.insert(text.to_string(), span);
        Ok(span)
    }

    /// `bytes` written once, however many tables hold them.
    pub(super) fn entry(&mut self, bytes: &[u8]) -> Result<Span, String> {
        let hash = {
            use std::hash::Hasher;
            let mut hasher = rustc_hash::FxHasher::default();
            hasher.write(bytes);
            hasher.finish()
        };
        if let Some(candidates) = self.entries.get(&hash) {
            for span in candidates {
                let start = span.offset as usize - HEADER_LEN;
                if &self.body[start..start + span.len as usize] == bytes {
                    return Ok(*span);
                }
            }
        }
        let span = self.append(bytes)?;
        self.entries.entry(hash).or_default().push(span);
        Ok(span)
    }

    /// A section holding `bytes`, found by `name`.
    pub(super) fn section(&mut self, name: &str, bytes: &[u8]) -> Result<(), String> {
        let span = self.append(bytes)?;
        self.sections.push((name.to_string(), span));
        Ok(())
    }

    /// The file: header, body, directory.
    pub(super) fn finish(self) -> Vec<u8> {
        let mut directory = Vec::new();
        directory.extend_from_slice(&(self.sections.len() as u32).to_le_bytes());
        for (name, span) in &self.sections {
            directory.extend_from_slice(&(name.len() as u16).to_le_bytes());
            directory.extend_from_slice(name.as_bytes());
            directory.extend_from_slice(&span.offset.to_le_bytes());
            directory.extend_from_slice(&span.len.to_le_bytes());
        }
        let mut file = Vec::with_capacity(HEADER_LEN + self.body.len() + directory.len());
        file.extend_from_slice(&MAGIC);
        file.extend_from_slice(&VERSION.to_le_bytes());
        file.extend_from_slice(&0u32.to_le_bytes());
        file.extend_from_slice(&(self.body.len() as u64).to_le_bytes());
        file.extend_from_slice(&(directory.len() as u64).to_le_bytes());
        file.extend_from_slice(&[0; 8]);
        file.extend_from_slice(&self.body);
        file.extend_from_slice(&directory);
        let sum = checksum(&file[HEADER_LEN..]);
        file[HEADER_LEN - 8..HEADER_LEN].copy_from_slice(&sum.to_le_bytes());
        file
    }
}

/// What a registry field is stored as: a section of its own.
pub(crate) trait Stored: Sized {
    fn load(reader: &Reader, section: &str) -> Result<Self, String>;
}

/// The bytes of a file, shared by everything read out of it.
#[derive(Clone)]
pub(crate) struct Buffer(Arc<Vec<u8>>);

impl std::ops::Deref for Buffer {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.0
    }
}

impl std::fmt::Debug for Buffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Buffer({} bytes)", self.0.len())
    }
}

/// Bytes either held by their owner or read in place from a file's buffer.
#[derive(Debug, Clone)]
pub(crate) enum Slice {
    Owned(Vec<u8>),
    Stored(Buffer, Range<usize>),
}

impl Default for Slice {
    fn default() -> Self {
        Slice::Owned(Vec::new())
    }
}

impl std::ops::Deref for Slice {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        match self {
            Slice::Owned(bytes) => bytes,
            Slice::Stored(buffer, range) => &buffer[range.clone()],
        }
    }
}

/// A file read: its bytes, and where its sections are.
pub(crate) struct Reader {
    buffer: Buffer,
    sections: FxHashMap<String, Span>,
}

/// Why a file wasn't read.
pub(crate) enum Unreadable {
    /// Not a file of this layout, or damaged: the cache rebuilds it.
    Damaged,
    /// Whole by its checksum, but its directory doesn't hold together:
    /// only a bug in the writer causes this.
    Inconsistent(String),
}

impl Reader {
    pub(super) fn new(file: Vec<u8>) -> Result<Reader, Unreadable> {
        let file_len = file.len();
        let header = file.get(..HEADER_LEN).ok_or(Unreadable::Damaged)?;
        if header[..8] != MAGIC || header[8..12] != VERSION.to_le_bytes() {
            return Err(Unreadable::Damaged);
        }
        let field = |from: usize| u64::from_le_bytes(header[from..from + 8].try_into().unwrap());
        let body_len = field(16);
        let directory_len = field(24);
        let sum = field(32);
        let expected = (HEADER_LEN as u64)
            .checked_add(body_len)
            .and_then(|end| end.checked_add(directory_len));
        if expected != Some(file_len as u64) || sum != checksum(&file[HEADER_LEN..]) {
            return Err(Unreadable::Damaged);
        }
        let directory_start = HEADER_LEN + body_len as usize;
        let directory = &file[directory_start..];
        let sections = parse_directory(directory, file_len)
            .ok_or_else(|| Unreadable::Inconsistent("its directory is cut short".to_string()))?;
        Ok(Reader {
            buffer: Buffer(Arc::new(file)),
            sections,
        })
    }

    pub(super) fn has_section(&self, name: &str) -> bool {
        self.sections.contains_key(name)
    }

    /// Section `name`, read in place.
    pub(super) fn slice(&self, name: &str) -> Result<Slice, String> {
        Ok(Slice::Stored(self.buffer.clone(), self.section(name)?))
    }

    pub(super) fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    /// The range of section `name`.
    pub(super) fn section(&self, name: &str) -> Result<Range<usize>, String> {
        self.sections
            .get(name)
            .map(|span| span.range())
            .ok_or_else(|| format!("it has no section {name}"))
    }
}

fn parse_directory(directory: &[u8], file_len: usize) -> Option<FxHashMap<String, Span>> {
    let mut at = 0usize;
    let take = |at: &mut usize, n: usize| -> Option<&[u8]> {
        let bytes = directory.get(*at..at.checked_add(n)?)?;
        *at += n;
        Some(bytes)
    };
    let count = u32::from_le_bytes(take(&mut at, 4)?.try_into().ok()?);
    let mut sections = FxHashMap::default();
    for _ in 0..count {
        let name_len = u16::from_le_bytes(take(&mut at, 2)?.try_into().ok()?) as usize;
        let name = std::str::from_utf8(take(&mut at, name_len)?)
            .ok()?
            .to_string();
        let offset = u32::from_le_bytes(take(&mut at, 4)?.try_into().ok()?);
        let len = u32::from_le_bytes(take(&mut at, 4)?.try_into().ok()?);
        let span = Span { offset, len };
        if span.range().end > file_len || (offset as usize) < HEADER_LEN {
            return None;
        }
        sections.insert(name, span);
    }
    Some(sections)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file() -> Vec<u8> {
        let mut writer = Writer::new();
        let shared = writer.entry(b"shared bytes").unwrap();
        assert_eq!(writer.entry(b"shared bytes").unwrap(), shared);
        assert_ne!(writer.entry(b"other bytes").unwrap(), shared);
        assert_eq!(writer.name("Name").unwrap(), writer.name("Name").unwrap());
        writer.section("one", b"first").unwrap();
        writer.section("two", b"").unwrap();
        writer.finish()
    }

    #[test]
    fn sections_read_back_where_they_were_written() {
        let reader = Reader::new(file()).ok().expect("a whole file reads");
        assert_eq!(&reader.buffer()[reader.section("one").unwrap()], b"first");
        assert_eq!(&reader.buffer()[reader.section("two").unwrap()], b"");
        assert!(reader.section("three").is_err());
    }

    #[test]
    fn equal_entries_are_written_once() {
        let mut writer = Writer::new();
        let a = writer.entry(&[7; 100]).unwrap();
        let b = writer.entry(&[7; 100]).unwrap();
        assert_eq!(a, b);
        assert_eq!(writer.body.len(), 100);
    }

    #[test]
    fn a_truncated_or_altered_file_is_damaged() {
        let whole = file();
        for cut in [0, 5, HEADER_LEN - 1, HEADER_LEN, whole.len() - 1] {
            assert!(
                matches!(Reader::new(whole[..cut].to_vec()), Err(Unreadable::Damaged)),
                "cut at {cut}"
            );
        }
        for at in [0, 9, 20, HEADER_LEN, whole.len() - 1] {
            let mut altered = whole.clone();
            altered[at] ^= 1;
            assert!(
                matches!(Reader::new(altered), Err(Unreadable::Damaged)),
                "byte {at}"
            );
        }
    }

    #[test]
    fn a_file_of_another_version_is_damaged() {
        let mut whole = file();
        whole[8] = whole[8].wrapping_add(1);
        assert!(matches!(Reader::new(whole), Err(Unreadable::Damaged)));
    }
}
