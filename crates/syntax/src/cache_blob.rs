//! The layout of a table of values a cache file keeps once each: the count,
//! the end of each value's bytes (little-endian `u32`s), then the bytes.

use std::ops::Range;

/// The values of one file, in the order they were pushed.
#[derive(Default)]
pub struct BlobWriter {
    ends: Vec<u32>,
    blob: Vec<u8>,
}

impl BlobWriter {
    /// Appends `bytes` as the next value and returns its index.
    pub fn push(&mut self, bytes: &[u8]) -> Result<u32, String> {
        let index = u32::try_from(self.ends.len())
            .map_err(|_| "a cache file has too many distinct values".to_string())?;
        self.blob.extend_from_slice(bytes);
        let end = u32::try_from(self.blob.len())
            .map_err(|_| "a cache file's values outgrew its address space".to_string())?;
        self.ends.push(end);
        Ok(index)
    }

    /// How many values were pushed.
    pub fn len(&self) -> usize {
        self.ends.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ends.is_empty()
    }

    /// The section's bytes.
    pub fn section(&self) -> Vec<u8> {
        section_of(&self.ends, &self.blob)
    }
}

/// The section for `ends` and `blob`; an empty one for no values.
pub fn section_of(ends: &[u32], blob: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + 4 * ends.len() + blob.len());
    out.extend_from_slice(&(ends.len() as u32).to_le_bytes());
    for end in ends {
        out.extend_from_slice(&end.to_le_bytes());
    }
    out.extend_from_slice(blob);
    out
}

/// Where each value's bytes lie in `buffer`, for the section at `range`.
pub fn parse_section(buffer: &[u8], range: Range<usize>) -> Result<Vec<Range<usize>>, String> {
    let section = buffer
        .get(range.clone())
        .ok_or("its table lies outside the file")?;
    let count = section
        .get(..4)
        .map(|bytes| u32::from_le_bytes(bytes.try_into().expect("four bytes")) as usize)
        .ok_or("its table has no count")?;
    let blob_start = count
        .checked_mul(4)
        .and_then(|ends| ends.checked_add(4))
        .filter(|start| *start <= section.len())
        .ok_or("its table is shorter than its count")?;
    let blob_len = section.len() - blob_start;
    let mut forms = Vec::with_capacity(count);
    let mut start = 0usize;
    for at in 0..count {
        let bytes = &section[4 + 4 * at..8 + 4 * at];
        let end = u32::from_le_bytes(bytes.try_into().expect("four bytes")) as usize;
        if end < start || end > blob_len {
            return Err("its table has a value out of order".to_string());
        }
        forms.push(range.start + blob_start + start..range.start + blob_start + end);
        start = end;
    }
    Ok(forms)
}
