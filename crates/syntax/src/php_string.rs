//! A PHP string value: a sequence of bytes, not necessarily UTF-8. `"\xff"`
//! is one byte, and a source file's non-UTF-8 bytes stay as written.

use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;

#[derive(
    Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Default, serde::Serialize, serde::Deserialize,
)]
pub struct PhpString(Arc<[u8]>);

#[cfg(feature = "fuzzing")]
impl<'a> arbitrary::Arbitrary<'a> for PhpString {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<PhpString> {
        stack::guard(|| Ok(PhpString(Arc::from(<&[u8]>::arbitrary(u)?))))
    }
}

impl PhpString {
    pub fn from_bytes(bytes: impl Into<Arc<[u8]>>) -> PhpString {
        PhpString(bytes.into())
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// The string as text, when its bytes are UTF-8.
    pub fn as_str(&self) -> Option<&str> {
        std::str::from_utf8(&self.0).ok()
    }

    /// The string as text, each invalid UTF-8 sequence replaced by U+FFFD.
    pub fn to_str_lossy(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.0)
    }

    /// The length in bytes, as `strlen()` counts it.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// `strtolower()`: ASCII letters only, as PHP 8 does.
    pub fn to_ascii_lowercase(&self) -> PhpString {
        PhpString::from(self.0.to_ascii_lowercase())
    }

    /// `strtoupper()`: ASCII letters only, as PHP 8 does.
    pub fn to_ascii_uppercase(&self) -> PhpString {
        PhpString::from(self.0.to_ascii_uppercase())
    }

    /// `str_repeat()`.
    pub fn repeat(&self, times: usize) -> PhpString {
        PhpString::from(self.0.repeat(times))
    }

    /// The `.` operator.
    pub fn concat(&self, other: &PhpString) -> PhpString {
        let mut bytes = Vec::with_capacity(self.len() + other.len());
        bytes.extend_from_slice(&self.0);
        bytes.extend_from_slice(&other.0);
        PhpString::from(bytes)
    }

    /// `trim()` with its default characters (space, tab, newline, carriage
    /// return, NUL, vertical tab).
    pub fn trim(&self) -> PhpString {
        let is_space = |b: &u8| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0 | 0x0B);
        let start = self
            .0
            .iter()
            .position(|b| !is_space(b))
            .unwrap_or(self.0.len());
        let end = self
            .0
            .iter()
            .rposition(|b| !is_space(b))
            .map_or(start, |i| i + 1);
        PhpString::from(self.0[start..end].to_vec())
    }
}

impl From<&str> for PhpString {
    fn from(s: &str) -> PhpString {
        PhpString(Arc::from(s.as_bytes()))
    }
}

impl From<String> for PhpString {
    fn from(s: String) -> PhpString {
        PhpString(Arc::from(s.into_bytes()))
    }
}

impl From<&String> for PhpString {
    fn from(s: &String) -> PhpString {
        PhpString::from(s.as_str())
    }
}

impl From<Arc<str>> for PhpString {
    fn from(s: Arc<str>) -> PhpString {
        PhpString::from(&*s)
    }
}

impl From<Vec<u8>> for PhpString {
    fn from(bytes: Vec<u8>) -> PhpString {
        PhpString(Arc::from(bytes))
    }
}

impl PartialEq<str> for PhpString {
    fn eq(&self, other: &str) -> bool {
        &*self.0 == other.as_bytes()
    }
}

impl PartialEq<&str> for PhpString {
    fn eq(&self, other: &&str) -> bool {
        &*self.0 == other.as_bytes()
    }
}

impl PartialEq<String> for PhpString {
    fn eq(&self, other: &String) -> bool {
        &*self.0 == other.as_bytes()
    }
}

impl fmt::Display for PhpString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_str_lossy())
    }
}

impl fmt::Debug for PhpString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.to_str_lossy())
    }
}
