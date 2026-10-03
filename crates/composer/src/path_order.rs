//! Sorting paths in the order `Path` compares them in, by a key made once
//! for each.
//!
//! `Path` compares component by component, so `a/b` comes before `a-b`
//! though `-` is a lower byte than `/`, and every comparison walks both
//! paths' components. The key is each component as a byte for its kind
//! (the order the kinds are declared in), its bytes and a zero byte, which
//! no file name holds: comparing keys as bytes is comparing the component
//! sequences.

use std::path::{Component, Path, PathBuf};

/// The key `Path`'s order is the byte order of.
fn key(path: &Path) -> Vec<u8> {
    let mut key = Vec::with_capacity(path.as_os_str().len() + 8);
    for component in path.components() {
        key.push(match component {
            Component::Prefix(_) => 1,
            Component::RootDir => 2,
            Component::CurDir => 3,
            Component::ParentDir => 4,
            Component::Normal(_) => 5,
        });
        key.extend_from_slice(component.as_os_str().as_encoded_bytes());
        key.push(0);
    }
    key
}

/// How many runs of paths already in order `sort_paths` merges instead of
/// making a key for each path.
const MERGED_RUNS: usize = 16;

/// `paths.sort()`, stable, without comparing component walks more than it
/// needs to: a list that is already in order, or is a few lists in order
/// one after the other (a walk of each directory, with its entries in
/// order, gives one), is merged by the paths themselves, and any other is
/// sorted by a key made once for each.
pub fn sort_paths(paths: &mut [PathBuf]) {
    let descents = paths.windows(2).filter(|pair| pair[1] < pair[0]).count();
    if descents == 0 {
        return;
    }
    if descents < MERGED_RUNS {
        paths.sort();
    } else {
        paths.sort_by_cached_key(|path| key(path));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A repeatable mix of paths that differ where byte order and component
    /// order differ: separators against `-`, `.` and a space, doubled and
    /// trailing separators, dot components, absolute and relative.
    fn paths(count: usize) -> Vec<PathBuf> {
        let parts = [
            "a",
            "a-b",
            "a.b",
            "a b",
            "ab",
            "b",
            ".",
            "..",
            "é",
            "a\u{10348}",
            "vendor",
            "",
        ];
        let mut state = 0x853C_49E6_748F_EA9Bu64;
        let mut next = move |bound: usize| {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            ((state >> 33) as usize) % bound
        };
        (0..count)
            .map(|_| {
                let mut text = match next(4) {
                    0 => String::from("/"),
                    1 => String::from("./"),
                    _ => String::new(),
                };
                for i in 0..1 + next(5) {
                    if i > 0 {
                        text.push_str(if next(6) == 0 { "//" } else { "/" });
                    }
                    text.push_str(parts[next(parts.len())]);
                }
                if next(5) == 0 {
                    text.push('/');
                }
                PathBuf::from(text)
            })
            .collect()
    }

    #[test]
    fn sorted_by_key_is_sorted_by_path() {
        let all = paths(3000);
        let mut by_key = all.clone();
        sort_paths(&mut by_key);
        let mut by_path = all;
        by_path.sort();
        assert_eq!(by_key, by_path);
    }

    #[test]
    fn the_key_order_is_the_path_order_for_every_pair() {
        let all = paths(300);
        for a in &all {
            for b in &all {
                assert_eq!(
                    key(a).cmp(&key(b)),
                    a.cmp(b),
                    "{} against {}",
                    a.display(),
                    b.display()
                );
            }
        }
    }

    #[test]
    fn a_separator_orders_before_a_dash_as_a_path_does() {
        let mut list = vec![PathBuf::from("a-b"), PathBuf::from("a/b")];
        sort_paths(&mut list);
        assert_eq!(list, [PathBuf::from("a/b"), PathBuf::from("a-b")]);
    }
}
