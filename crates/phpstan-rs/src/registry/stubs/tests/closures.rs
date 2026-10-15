//! A stub file's closure is every stub file reached from it through the
//! names it declares: the files that reference one of them, the files that
//! reference a name one of those declares, and so on, whatever shape the
//! references between the files take.

use std::sync::Arc;

use proptest::prelude::*;

use super::super::{Corpus, DeclaredKeys, Packed, StubIndex};
use crate::registry::stub_set::Stubs;

fn corpus() -> Arc<Corpus> {
    Stubs::shared().corpus()
}

/// Stub files by number: the keys each declares and the keys each
/// references.
#[derive(Debug)]
struct Files {
    declares: Vec<Vec<u32>>,
    references: Vec<Vec<u32>>,
}

impl Files {
    /// Files `0..count`, each declaring a key of its own, where `to`
    /// references the key of `from` for each `(from, to)` of `leads`.
    fn leading(count: usize, leads: &[(usize, usize)]) -> Files {
        let mut references = vec![Vec::new(); count];
        for &(from, to) in leads {
            references[to].push(from as u32);
        }
        Files {
            declares: (0..count as u32).map(|key| vec![key]).collect(),
            references,
        }
    }

    /// The closures `build_closures` makes, each as words.
    fn built(&self) -> Vec<Vec<u64>> {
        let mut dependents: rustc_hash::FxHashMap<String, Vec<u8>> = Default::default();
        for (file, keys) in self.references.iter().enumerate() {
            for key in keys {
                dependents
                    .entry(format!("k{key}"))
                    .or_default()
                    .extend_from_slice(&(file as u32).to_le_bytes());
            }
        }
        let index = StubIndex {
            dependents: Packed::build(dependents.into_iter().collect()),
            ..StubIndex::default()
        };
        let keys: Vec<DeclaredKeys> = self
            .declares
            .iter()
            .map(|keys| {
                keys.iter()
                    .map(|key| (false, Box::from(format!("k{key}"))))
                    .collect()
            })
            .collect();
        let closures = index.build_closures(&keys);
        (0..closures.len())
            .map(|file| {
                closures
                    .get(file)
                    .as_chunks::<8>()
                    .0
                    .iter()
                    .map(|word| u64::from_le_bytes(*word))
                    .collect()
            })
            .collect()
    }

    /// The files reached from `file`, itself included, by looking up the
    /// files that reference each key a reached file declares, one key at a
    /// time, as words.
    fn reached(&self, file: usize) -> Vec<u64> {
        let mut referencing: rustc_hash::FxHashMap<u32, Vec<usize>> = Default::default();
        for (dependent, keys) in self.references.iter().enumerate() {
            for &key in keys {
                referencing.entry(key).or_default().push(dependent);
            }
        }
        let mut words = vec![0u64; self.declares.len().div_ceil(64)];
        words[file / 64] |= 1 << (file % 64);
        let mut stack = vec![file];
        while let Some(current) = stack.pop() {
            for key in &self.declares[current] {
                for &dependent in referencing.get(key).map_or(&[][..], Vec::as_slice) {
                    let (word, bit) = (dependent / 64, 1u64 << (dependent % 64));
                    if words[word] & bit == 0 {
                        words[word] |= bit;
                        stack.push(dependent);
                    }
                }
            }
        }
        words
    }

    fn assert_closures(&self) {
        let built = self.built();
        assert_eq!(built.len(), self.declares.len(), "{self:?}");
        for (file, closure) in built.iter().enumerate() {
            assert_eq!(closure, &self.reached(file), "file {file} of {self:?}");
        }
    }
}

#[test]
fn a_stub_file_closure_is_every_file_reached_through_the_names_it_declares() {
    let corpus = corpus();
    let count = corpus.stub_file_count();
    // The files the key-by-key walk reaches from `file`, as words.
    let by_keys = |file: u32| {
        let mut reached = vec![0u64; count.div_ceil(64)];
        reached[file as usize / 64] |= 1 << (file % 64);
        let mut stack = vec![file];
        while let Some(current) = stack.pop() {
            for (_, name) in corpus.stub_file_names(current) {
                for dependent in corpus.stub_dependent_files(name) {
                    let (word, bit) = (dependent as usize / 64, 1u64 << (dependent % 64));
                    if reached[word] & bit == 0 {
                        reached[word] |= bit;
                        stack.push(dependent);
                    }
                }
            }
        }
        reached
    };
    for file in 0..count as u32 {
        assert_eq!(corpus.stub_file_closure(file), by_keys(file), "file {file}");
    }
}

#[test]
fn closures_follow_cycles_chains_and_shared_files() {
    // One file alone, and one that references its own name.
    Files::leading(1, &[]).assert_closures();
    Files::leading(1, &[(0, 0)]).assert_closures();
    // A chain into a cycle, beside a file nothing reaches.
    Files::leading(5, &[(0, 1), (1, 2), (2, 3), (3, 1)]).assert_closures();
    // A cycle leading to another, which leads on to a last file.
    Files::leading(5, &[(0, 1), (1, 0), (1, 2), (2, 3), (3, 2), (3, 4)]).assert_closures();
    // A diamond.
    Files::leading(4, &[(0, 1), (0, 2), (1, 3), (2, 3)]).assert_closures();
    // Files that reach, from a later start or a later branch, a cycle or a
    // file whose closure is already made.
    Files::leading(6, &[(0, 1), (1, 2), (2, 1), (0, 3), (3, 2), (4, 0), (5, 3)]).assert_closures();
    Files::leading(4, &[(3, 0), (0, 1), (1, 0), (2, 1)]).assert_closures();
    // A key that two files declare leads from both of them.
    Files {
        declares: vec![vec![0], vec![0], vec![]],
        references: vec![vec![], vec![], vec![0]],
    }
    .assert_closures();
    // Closures over several words: a chain across them, closed into a
    // cycle that starts in the second.
    let mut leads: Vec<(usize, usize)> = (0..129).map(|file| (file, file + 1)).collect();
    leads.push((129, 64));
    Files::leading(130, &leads).assert_closures();
    // A long chain: each file's closure is itself and the files after it.
    let leads: Vec<(usize, usize)> = (0..2999).map(|file| (file, file + 1)).collect();
    Files::leading(3000, &leads).assert_closures();
}

/// Between 1 and 200 files, declaring and referencing keys out of a pool of
/// 1 to 400: a small pool makes dense graphs with large cycles, a large one
/// sparse graphs.
fn files() -> impl Strategy<Value = Files> {
    (1usize..=200, 1u32..=400)
        .prop_flat_map(|(count, keys)| {
            let lists =
                move || proptest::collection::vec(proptest::collection::vec(0..keys, 0..4), count);
            (lists(), lists())
        })
        .prop_map(|(declares, references)| Files {
            declares,
            references,
        })
}

proptest! {
    #[test]
    fn closures_of_random_files_are_the_files_each_reaches(files in files()) {
        files.assert_closures();
    }
}
