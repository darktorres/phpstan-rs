//! The cache is a header, eager sections and independently checked records
//! in a body, and a section directory. Readers keep eager sections and read
//! record payloads from the open file only when a table first needs them.

use std::fs::File;
use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rustc_hash::FxHashMap;

const MAGIC: [u8; 8] = *b"PRSSTUBS";
pub(super) const NAMES: &str = "__names";

/// Magic, the body's length, the directory's length, the checksum. The
/// cache key changes with the code that writes this layout, so a manual
/// version number would duplicate that identity and could drift out of sync.
pub(super) const HEADER_LEN: usize = 8 + 8 + 8 + 8;

/// The header checksum covers eager sections and the directory. Lazy record
/// payloads carry their own checksums in their table records.
fn checksum_parts<'a>(parts: impl IntoIterator<Item = &'a [u8]>) -> u64 {
    let mut checksum = crc32fast::Hasher::new();
    for part in parts {
        checksum.update(part);
    }
    u64::from(checksum.finalize())
}

/// Where bytes sit in the file: the offset from its start and the length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Span {
    pub(super) offset: u64,
    pub(super) len: u64,
}

impl Span {
    fn body_range(self) -> Range<usize> {
        let start = usize::try_from(self.offset).expect("writer span fits memory") - HEADER_LEN;
        start..start + usize::try_from(self.len).expect("writer span fits memory")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct StoredEntry {
    pub(super) span: Span,
    pub(super) checksum: u32,
}

/// Builds a file.
pub(crate) struct Writer {
    body: Vec<u8>,
    sections: Vec<(String, Span)>,
    // Names in every section and serialized record refer to this per-file
    // table by index. Encoded entries are still deduplicated by their bytes.
    names: syntax::name::CacheNameEncoder,
    entries: FxHashMap<u64, Vec<StoredEntry>>,
}

impl Writer {
    pub(crate) fn new() -> Writer {
        Writer {
            body: Vec::new(),
            sections: Vec::new(),
            names: syntax::name::CacheNameEncoder::default(),
            entries: FxHashMap::default(),
        }
    }

    #[cfg(test)]
    pub(super) fn body_len(&self) -> usize {
        self.body.len()
    }

    fn append(&mut self, bytes: &[u8]) -> Result<Span, String> {
        let offset = u64::try_from(HEADER_LEN + self.body.len())
            .map_err(|_| "the stored stubs outgrew the address space".to_string())?;
        let len = u64::try_from(bytes.len())
            .map_err(|_| "a stored entry outgrew the address space".to_string())?;
        self.body.extend_from_slice(bytes);
        u64::try_from(HEADER_LEN + self.body.len())
            .map_err(|_| "the stored stubs outgrew the address space".to_string())?;
        Ok(Span { offset, len })
    }

    /// The index of `text` in this file's name table.
    pub(super) fn name(&mut self, text: &str) -> Result<u32, String> {
        self.names.index(text)
    }

    /// Encodes a value with this file's shared name table.
    pub(crate) fn encode<T: serde::Serialize>(&self, value: &T) -> Result<Vec<u8>, String> {
        syntax::name::with_cache_name_encoder(&self.names, || {
            postcard::to_allocvec(value).map_err(|error| error.to_string())
        })
    }

    /// `bytes` written once, however many tables hold them.
    pub(super) fn entry(&mut self, bytes: &[u8]) -> Result<StoredEntry, String> {
        let hash = {
            use std::hash::Hasher;
            let mut hasher = rustc_hash::FxHasher::default();
            hasher.write(bytes);
            hasher.finish()
        };
        if let Some(candidates) = self.entries.get(&hash) {
            for entry in candidates {
                if &self.body[entry.span.body_range()] == bytes {
                    return Ok(*entry);
                }
            }
        }
        let entry = StoredEntry {
            span: self.append(bytes)?,
            checksum: crc32fast::hash(bytes),
        };
        self.entries.entry(hash).or_default().push(entry);
        Ok(entry)
    }

    /// A section holding `bytes`, found by `name`.
    pub(crate) fn section(&mut self, name: &str, bytes: &[u8]) -> Result<(), String> {
        let span = self.append(bytes)?;
        self.sections.push((name.to_string(), span));
        Ok(())
    }

    /// The file: header, body, directory.
    pub(crate) fn finish(mut self) -> Vec<u8> {
        let names = postcard::to_allocvec(&self.names.names())
            .expect("a cache file's name table always serializes");
        let names_span = self
            .append(&names)
            .expect("a cache file's name table fits its address space");
        self.sections.push((NAMES.to_string(), names_span));
        let mut directory = Vec::new();
        directory.extend_from_slice(&(self.sections.len() as u32).to_le_bytes());
        for (name, span) in &self.sections {
            directory.extend_from_slice(&(name.len() as u16).to_le_bytes());
            directory.extend_from_slice(name.as_bytes());
            directory.extend_from_slice(&span.offset.to_le_bytes());
            directory.extend_from_slice(&span.len.to_le_bytes());
        }
        let mut eager_spans: Vec<_> = self.sections.iter().map(|(_, span)| *span).collect();
        eager_spans.sort_unstable_by_key(|span| span.offset);
        let eager_checksum = checksum_parts(
            eager_spans
                .iter()
                .map(|span| &self.body[span.body_range()])
                .chain(std::iter::once(directory.as_slice())),
        );
        let mut file = Vec::with_capacity(HEADER_LEN + self.body.len() + directory.len());
        file.extend_from_slice(&MAGIC);
        file.extend_from_slice(&(self.body.len() as u64).to_le_bytes());
        file.extend_from_slice(&(directory.len() as u64).to_le_bytes());
        file.extend_from_slice(&eager_checksum.to_le_bytes());
        file.extend_from_slice(&self.body);
        file.extend_from_slice(&directory);
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

#[derive(Clone)]
pub(super) enum Source {
    Memory(Arc<Vec<u8>>),
    File {
        file: Arc<File>,
        path: Option<Arc<PathBuf>>,
        identity: Option<u128>,
    },
}

impl Source {
    pub(super) fn read(&self, offset: u64, len: u64) -> Result<Vec<u8>, String> {
        let len = usize::try_from(len).map_err(|_| "a cache range is too large".to_string())?;
        let mut bytes = vec![0; len];
        match self {
            Source::Memory(file) => {
                let start = usize::try_from(offset)
                    .map_err(|_| "a cache offset is too large".to_string())?;
                let end = start
                    .checked_add(len)
                    .ok_or_else(|| "a cache range overflows the address space".to_string())?;
                bytes.copy_from_slice(
                    file.get(start..end)
                        .ok_or_else(|| "a cache range points outside its file".to_string())?,
                );
            }
            Source::File { file, .. } => read_exact_at(file, offset, &mut bytes)
                .map_err(|error| format!("reading cache record at {offset}: {error}"))?,
        }
        Ok(bytes)
    }

    pub(super) fn invalidate(&self) {
        if let Source::File {
            file,
            path: Some(path),
            identity: Some(identity),
        } = self
        {
            // A lazy read can fail after another process has already replaced
            // the cache at this path. Only unlink the file this Reader opened;
            // otherwise a stale reader could delete the healthy replacement.
            if std::fs::metadata(path.as_ref())
                .ok()
                .and_then(|metadata| file_identity(&metadata))
                == Some(*identity)
                && file
                    .metadata()
                    .ok()
                    .and_then(|metadata| file_identity(&metadata))
                    == Some(*identity)
            {
                let _ = std::fs::remove_file(path.as_ref());
            }
        }
    }

    pub(super) fn read_entry(&self, entry: StoredEntry) -> Result<Vec<u8>, String> {
        let bytes = match self.read(entry.span.offset, entry.span.len) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.invalidate();
                return Err(error);
            }
        };
        if crc32fast::hash(&bytes) != entry.checksum {
            self.invalidate();
            return Err(format!(
                "cache record at {} failed its checksum",
                entry.span.offset
            ));
        }
        Ok(bytes)
    }
}

#[cfg(unix)]
fn read_exact_at(file: &File, mut offset: u64, mut bytes: &mut [u8]) -> io::Result<()> {
    use std::os::unix::fs::FileExt;
    while !bytes.is_empty() {
        match file.read_at(bytes, offset) {
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(read) => {
                offset += read as u64;
                bytes = &mut bytes[read..];
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[cfg(windows)]
fn read_exact_at(file: &File, mut offset: u64, mut bytes: &mut [u8]) -> io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !bytes.is_empty() {
        match file.seek_read(bytes, offset) {
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(read) => {
                offset += read as u64;
                bytes = &mut bytes[read..];
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn read_exact_at(_file: &File, _offset: u64, _bytes: &mut [u8]) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "positional cache reads are unsupported on this platform",
    ))
}

/// A file read: eager sections are buffered, lazy records stay at file offsets.
pub(crate) struct Reader {
    buffer: Buffer,
    source: Source,
    sections: FxHashMap<String, Range<usize>>,
    names: syntax::name::CacheNameTable,
    decoded: Arc<DecodedEntries>,
}

/// The entries read out of a file so far, by the type they decoded to and
/// where their bytes are, for the tables that store the same bytes to share.
pub(super) type DecodedEntries =
    Mutex<FxHashMap<(std::any::TypeId, u64), Arc<dyn std::any::Any + Send + Sync>>>;

/// Why a file wasn't read.
#[derive(Debug)]
pub(crate) enum Unreadable {
    /// Not a file of this layout, or damaged: the cache rebuilds it.
    Damaged,
    /// Whole by its checksum, but its directory doesn't hold together:
    /// only a bug in the writer causes this.
    Inconsistent(String),
}

impl Reader {
    pub(crate) fn new(file: Vec<u8>) -> Result<Reader, Unreadable> {
        let file = Arc::new(file);
        let file_len = file.len() as u64;
        Self::read(Source::Memory(Arc::clone(&file)), file_len)
    }

    pub(crate) fn open(path: &Path) -> Result<Reader, Unreadable> {
        let file = Arc::new(File::open(path).map_err(|_| Unreadable::Damaged)?);
        let file_len = file.metadata().map_err(|_| Unreadable::Damaged)?.len();
        Self::read(
            Source::File {
                identity: file_identity(&file.metadata().map_err(|_| Unreadable::Damaged)?),
                file,
                path: Some(Arc::new(path.to_path_buf())),
            },
            file_len,
        )
    }

    fn read(source: Source, file_len: u64) -> Result<Reader, Unreadable> {
        let header = source
            .read(0, HEADER_LEN as u64)
            .map_err(|_| Unreadable::Damaged)?;
        if header[..8] != MAGIC {
            return Err(Unreadable::Damaged);
        }
        let field = |from: usize| u64::from_le_bytes(header[from..from + 8].try_into().unwrap());
        let body_len = field(8);
        let directory_len = field(16);
        let sum = field(24);
        let expected = (HEADER_LEN as u64)
            .checked_add(body_len)
            .and_then(|end| end.checked_add(directory_len));
        if expected != Some(file_len) {
            return Err(Unreadable::Damaged);
        }
        let body_end = (HEADER_LEN as u64)
            .checked_add(body_len)
            .ok_or(Unreadable::Damaged)?;
        let directory = source
            .read(body_end, directory_len)
            .map_err(|_| Unreadable::Damaged)?;
        let spans = parse_directory(&directory, body_end, file_len).ok_or(Unreadable::Damaged)?;
        if !spans.contains_key(NAMES) {
            return Err(Unreadable::Inconsistent("it has no name table".to_string()));
        }
        let mut ordered: Vec<_> = spans.iter().collect();
        ordered.sort_unstable_by_key(|(_, span)| span.offset);
        let mut eager_bytes = Vec::new();
        let mut sections = FxHashMap::default();
        let mut eager_crc = crc32fast::Hasher::new();
        for (name, span) in ordered {
            let section_bytes = source
                .read(span.offset, span.len)
                .map_err(|_| Unreadable::Damaged)?;
            eager_crc.update(&section_bytes);
            let start = eager_bytes.len();
            eager_bytes.extend_from_slice(&section_bytes);
            sections.insert(name.clone(), start..eager_bytes.len());
        }
        eager_crc.update(&directory);
        if sum != u64::from(eager_crc.finalize()) {
            return Err(Unreadable::Damaged);
        }
        let names_range = sections[NAMES].clone();
        let names =
            postcard::from_bytes::<Vec<String>>(&eager_bytes[names_range]).map_err(|error| {
                Unreadable::Inconsistent(format!("its name table does not decode: {error}"))
            })?;
        Ok(Reader {
            buffer: Buffer(Arc::new(eager_bytes)),
            source,
            sections,
            names: syntax::name::CacheNameTable::new(names),
            decoded: Arc::default(),
        })
    }

    /// Section `name`, read in place.
    pub(super) fn slice(&self, name: &str) -> Result<Slice, String> {
        Ok(Slice::Stored(self.buffer.clone(), self.section(name)?))
    }

    pub(crate) fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    pub(crate) fn names(&self) -> &syntax::name::CacheNameTable {
        &self.names
    }

    pub(crate) fn set_cache_context(&mut self, context: u64) {
        self.names.set_context(context);
    }

    pub(super) fn decoded(&self) -> &Arc<DecodedEntries> {
        &self.decoded
    }

    pub(super) fn source(&self) -> Source {
        self.source.clone()
    }

    /// The range of section `name`.
    pub(crate) fn section(&self, name: &str) -> Result<Range<usize>, String> {
        self.sections
            .get(name)
            .cloned()
            .ok_or_else(|| format!("it has no section {name}"))
    }
}

#[cfg(unix)]
fn file_identity(metadata: &std::fs::Metadata) -> Option<u128> {
    use std::os::unix::fs::MetadataExt;
    Some((u128::from(metadata.dev()) << 64) | u128::from(metadata.ino()))
}

// The standard library does not expose a stable cross-platform identity for
// an open file on every target. In that case, preserve replacements rather
// than risk unlinking a different cache file.
#[cfg(not(unix))]
fn file_identity(_metadata: &std::fs::Metadata) -> Option<u128> {
    None
}

fn parse_directory(
    directory: &[u8],
    body_end: u64,
    file_len: u64,
) -> Option<FxHashMap<String, Span>> {
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
        let offset = u64::from_le_bytes(take(&mut at, 8)?.try_into().ok()?);
        let len = u64::from_le_bytes(take(&mut at, 8)?.try_into().ok()?);
        let span = Span { offset, len };
        let end = span.offset.checked_add(span.len)?;
        if span.offset < HEADER_LEN as u64 || end > body_end || end > file_len {
            return None;
        }
        if sections.insert(name, span).is_some() {
            return None;
        }
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
        let reader = Reader::new(file()).expect("a whole file reads");
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
        for at in [0, 9, 20, whole.len() - 1] {
            let mut altered = whole.clone();
            altered[at] ^= 1;
            assert!(
                matches!(Reader::new(altered), Err(Unreadable::Damaged)),
                "byte {at}"
            );
        }
    }

    #[test]
    fn a_file_with_other_magic_is_damaged() {
        let mut whole = file();
        whole[0] = whole[0].wrapping_add(1);
        assert!(matches!(Reader::new(whole), Err(Unreadable::Damaged)));
    }

    #[test]
    fn a_lazy_record_checksum_is_checked_when_read() {
        let mut writer = Writer::new();
        let entry = writer.entry(b"payload").unwrap();
        let mut whole = writer.finish();
        whole[entry.span.offset as usize] ^= 1;
        let reader = Reader::new(whole).expect("lazy entry bytes are checked on demand");
        assert!(reader.source.read_entry(entry).is_err());
    }

    #[test]
    fn an_open_reader_keeps_reading_the_replaced_file() {
        let directory = std::env::temp_dir().join(format!(
            "phpstan-rs-cache-replace-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("cache.bin");
        let replacement = directory.join("replacement.bin");

        let mut old_writer = Writer::new();
        let old_entry = old_writer.entry(b"old payload").unwrap();
        std::fs::write(&path, old_writer.finish()).unwrap();
        let old_reader = Reader::open(&path).expect("opens the old file");

        let mut new_writer = Writer::new();
        new_writer.section("value", b"new payload").unwrap();
        std::fs::write(&replacement, new_writer.finish()).unwrap();
        std::fs::rename(&replacement, &path).expect("replaces an open cache file");

        assert_eq!(
            old_reader.source.read_entry(old_entry).unwrap(),
            b"old payload"
        );
        let new_reader = Reader::open(&path).expect("opens the replacement");
        let section = new_reader.section("value").unwrap();
        assert_eq!(&new_reader.buffer()[section], b"new payload");
        drop((old_reader, new_reader));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_corrupt_old_reader_does_not_remove_a_replacement() {
        let directory = std::env::temp_dir().join(format!(
            "phpstan-rs-cache-corrupt-replace-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("cache.bin");
        let replacement = directory.join("replacement.bin");

        let mut old_writer = Writer::new();
        let old_entry = old_writer.entry(b"old payload").unwrap();
        let mut old_bytes = old_writer.finish();
        old_bytes[old_entry.span.offset as usize] ^= 1;
        std::fs::write(&path, old_bytes).unwrap();
        let old_reader = Reader::open(&path).expect("opens the corrupt lazy record");

        let mut new_writer = Writer::new();
        new_writer.section("value", b"new payload").unwrap();
        std::fs::write(&replacement, new_writer.finish()).unwrap();
        std::fs::rename(&replacement, &path).expect("replaces the open cache file");

        assert!(old_reader.source.read_entry(old_entry).is_err());
        let new_reader = Reader::open(&path).expect("the replacement remains present");
        let section = new_reader.section("value").unwrap();
        assert_eq!(&new_reader.buffer()[section], b"new payload");
        drop((old_reader, new_reader));
        std::fs::remove_dir_all(directory).unwrap();
    }
}
