//! Git's object ids in its SHA-256 object format.

use std::fmt;
use std::io;
use std::str::FromStr;

use sha2::{Digest, Sha256};

use crate::invalid;

/// The id git gives an object in the SHA-256 object format: the SHA-256 of
/// the object's kind, its length and its content.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Id(pub [u8; 32]);

impl Id {
    /// The id of the blob holding `text`.
    pub fn blob(text: &[u8]) -> Id {
        object("blob", text)
    }

    /// The id of the tree whose content, its entries as git encodes them,
    /// is `content`.
    pub(crate) fn tree(content: &[u8]) -> Id {
        object("tree", content)
    }
}

fn object(kind: &str, content: &[u8]) -> Id {
    let mut hasher = Sha256::new();
    hasher.update(format!("{kind} {}\0", content.len()).as_bytes());
    hasher.update(content);
    Id(hasher.finalize().into())
}

/// Lowercase hex, as git prints an id.
impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Id(bytes) = self;
        for byte in bytes {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl FromStr for Id {
    type Err = io::Error;

    fn from_str(text: &str) -> io::Result<Id> {
        let digits = text.as_bytes();
        if digits.len() != 64 {
            return Err(invalid(format!("not an object id: {text:?}")));
        }
        let digit = |byte: u8| match byte {
            b'0'..=b'9' => Ok(byte - b'0'),
            b'a'..=b'f' => Ok(byte - b'a' + 10),
            _ => Err(invalid(format!("not an object id: {text:?}"))),
        };
        let mut bytes = [0; 32];
        for (byte, [high, low]) in bytes.iter_mut().zip(digits.as_chunks::<2>().0) {
            *byte = (digit(*high)? << 4) | digit(*low)?;
        }
        Ok(Id(bytes))
    }
}
