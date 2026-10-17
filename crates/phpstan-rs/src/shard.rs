//! The shards a table is split into for a merge.
//!
//! Each key of a table routes to one shard by a hash of its own, so the shards
//! of a table hold disjoint keys and one thread can write each of them. The
//! count is fixed, so a table's layout does not depend on the number of
//! threads.
//!
//! A table indexes its slots by the low bits of a key's hash and tags them by
//! the top seven (`hashbrown`). A shard takes bits between the two, so the
//! table a shard holds spreads its keys over its slots and tags them as the
//! whole table would. The top bits would give every key of a shard one tag.

use std::hash::{Hash, Hasher};

use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHasher};

const SHARD_BITS: u32 = 6;

/// The number of shards a table is split into.
pub(crate) const SHARDS: usize = 1 << SHARD_BITS;

/// The lowest bit a shard is routed by. Bits below it are the ones a table
/// indexes by, and the shard's bits end below the top seven.
const SHARD_SHIFT: u32 = 50;

/// The shard a hash routes to: the `SHARD_BITS` bits from `SHARD_SHIFT` up.
pub(crate) fn shard_of_hash(hash: u64) -> usize {
    ((hash >> SHARD_SHIFT) as usize) & (SHARDS - 1)
}

/// The shard a key of an `FxHashMap` routes to, by the hash the map gives it.
pub(crate) fn shard_of_key<K: Hash + ?Sized>(key: &K) -> usize {
    let mut hasher = FxHasher::default();
    key.hash(&mut hasher);
    shard_of_hash(hasher.finish())
}

/// Splits `map` into `SHARDS` maps, each holding the entries whose keys
/// `shard_of_key` routes to it.
pub(crate) fn split_map<K: Hash + Eq, V>(map: FxHashMap<K, V>) -> Vec<FxHashMap<K, V>> {
    let mut shards: Vec<FxHashMap<K, V>> = (0..SHARDS).map(|_| FxHashMap::default()).collect();
    for (key, value) in map {
        shards[shard_of_key(&key)].insert(key, value);
    }
    shards
}

/// Joins the maps `split_map` made back into one. A key is in one shard only,
/// so no entry is replaced.
pub(crate) fn join_map<K: Hash + Eq, V>(shards: Vec<FxHashMap<K, V>>) -> FxHashMap<K, V> {
    let mut joined = FxHashMap::default();
    for shard in shards {
        for (key, value) in shard {
            let previous = joined.insert(key, value);
            debug_assert!(previous.is_none(), "a key is in one shard only");
        }
    }
    joined
}

/// Merges `items` into `shards` in two parallel steps. `bucket` takes a chunk
/// of consecutive items and returns the work it makes for each shard, in shard
/// order, and runs over the chunks in parallel. Each shard then applies the
/// work of every chunk in the order of the chunks, so it sees the items in
/// their order, and the shards run in parallel.
pub(crate) fn merge_by_shard<T, W, S>(
    pool: &rayon::ThreadPool,
    shards: Vec<S>,
    items: Vec<T>,
    bucket: impl Fn(Vec<T>) -> Vec<W> + Sync,
    apply: impl Fn(&mut S, W) + Sync,
) -> Vec<S>
where
    T: Send,
    W: Send,
    S: Send,
{
    let chunks = into_chunks(items, pool.current_num_threads() * 4);
    let work = pool.install(|| {
        let buckets: Vec<Vec<W>> = chunks
            .into_par_iter()
            .map(|chunk| {
                let work = bucket(chunk);
                debug_assert_eq!(work.len(), SHARDS);
                work
            })
            .collect();
        by_shard(buckets)
    });
    pool.install(|| {
        shards
            .into_par_iter()
            .zip(work.into_par_iter())
            .map(|(mut shard, works)| {
                for work in works {
                    apply(&mut shard, work);
                }
                shard
            })
            .collect()
    })
}

/// `items` in at most `count` chunks of consecutive items, in order.
fn into_chunks<T>(items: Vec<T>, count: usize) -> Vec<Vec<T>> {
    let size = items.len().div_ceil(count).max(1);
    let mut items = items.into_iter().peekable();
    let mut chunks = Vec::new();
    while items.peek().is_some() {
        chunks.push(items.by_ref().take(size).collect());
    }
    chunks
}

/// The work of each shard, chunk after chunk in the order of the chunks.
fn by_shard<W>(chunks: Vec<Vec<W>>) -> Vec<Vec<W>> {
    let mut shards: Vec<Vec<W>> = (0..SHARDS)
        .map(|_| Vec::with_capacity(chunks.len()))
        .collect();
    for chunk in chunks {
        for (shard, work) in chunk.into_iter().enumerate() {
            shards[shard].push(work);
        }
    }
    shards
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_hash_routes_to_a_shard_below_the_count() {
        for hash in [0, 1, u64::MAX / 3, u64::MAX] {
            assert!(shard_of_hash(hash) < SHARDS);
        }
        assert_eq!(shard_of_hash(0), 0);
        assert_eq!(shard_of_hash(u64::MAX), SHARDS - 1);
        assert_eq!(shard_of_hash(1 << SHARD_SHIFT), 1);
    }

    #[test]
    fn a_shard_ignores_the_bits_a_table_indexes_and_tags_by() {
        let hash = 0x2a5c_9e31_f0d4_7b68;
        let shard = shard_of_hash(hash);
        assert_eq!(shard_of_hash(hash ^ 0xffff_ffff), shard);
        assert_eq!(shard_of_hash(hash ^ (0x7f << 57)), shard);
    }

    #[test]
    fn a_sharded_merge_gives_each_shard_its_items_in_their_order() {
        fn spread(item: u64) -> u64 {
            item.wrapping_mul(0x9e37_79b9_7f4a_7c15)
        }
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(3)
            .build()
            .expect("a pool");
        let shards: Vec<Vec<u64>> = (0..SHARDS).map(|_| Vec::new()).collect();
        let merged = merge_by_shard(
            &pool,
            shards,
            (0..1000).collect(),
            |chunk: Vec<u64>| {
                let mut work: Vec<Vec<u64>> = (0..SHARDS).map(|_| Vec::new()).collect();
                for item in chunk {
                    work[shard_of_hash(spread(item))].push(item);
                }
                work
            },
            |shard, work| shard.extend(work),
        );
        assert!(merged.iter().filter(|shard| !shard.is_empty()).count() > 1);
        let mut all: Vec<u64> = Vec::new();
        for (index, shard) in merged.iter().enumerate() {
            assert!(
                shard
                    .iter()
                    .all(|&item| shard_of_hash(spread(item)) == index)
            );
            assert!(shard.windows(2).all(|pair| pair[0] < pair[1]));
            all.extend(shard);
        }
        all.sort();
        assert_eq!(all, (0..1000).collect::<Vec<u64>>());
    }

    #[test]
    fn a_split_map_joins_back_to_the_same_map() {
        let map: FxHashMap<String, u32> = (0..1000).map(|n| (format!("name{n}"), n)).collect();
        let shards = split_map(map.clone());
        assert_eq!(shards.len(), SHARDS);
        assert!(shards.iter().filter(|shard| !shard.is_empty()).count() > 1);
        for (index, shard) in shards.iter().enumerate() {
            assert!(shard.keys().all(|key| shard_of_key(key) == index));
        }
        assert_eq!(join_map(shards), map);
    }
}
