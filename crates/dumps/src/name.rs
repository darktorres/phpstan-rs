//! An entity's name in its dump's tree.

use std::fmt::Write as _;

/// `key` as one component of an entity's path: as it is, but `%`, `/`,
/// `~`, `"`, the ASCII control bytes, every byte outside UTF-8 and a
/// leading `.` as `%NN`, and an empty key as `%`. A key's component then
/// never holds the path's separator, nor the suffix that tells a repeated
/// key apart (`~2`), nor starts as the names of a dump's own entries do
/// (`.0`, `.order`); and git takes it as a name as it is, `.git`, `..` and
/// a quote at the start included.
pub fn escape(key: &[u8]) -> String {
    if key.is_empty() {
        return "%".to_string();
    }
    let mut out = String::with_capacity(key.len());
    let mut first = true;
    for chunk in key.utf8_chunks() {
        for ch in chunk.valid().chars() {
            let plain = !matches!(ch, '%' | '/' | '~' | '"' | '\0'..='\x1f' | '\x7f')
                && !(first && ch == '.');
            if plain {
                out.push(ch);
            } else {
                let _ = write!(out, "%{:02X}", ch as u32);
            }
            first = false;
        }
        for byte in chunk.invalid() {
            let _ = write!(out, "%{byte:02X}");
            first = false;
        }
    }
    out
}
