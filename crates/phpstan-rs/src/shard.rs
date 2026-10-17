//! The shards a table is split into for a merge.
//!
//! Each key of a table routes to one shard by a hash of its own, so the shards
//! of a table hold disjoint keys and one thread can write each of them. The
//! count is fixed, so a table's layout does not depend on the number of
//! threads.

const SHARD_BITS: u32 = 6;

/// The number of shards a table is split into.
pub(crate) const SHARDS: usize = 1 << SHARD_BITS;

/// The shard a hash routes to: its top bits.
pub(crate) fn shard_of_hash(hash: u64) -> usize {
    (hash >> (u64::BITS - SHARD_BITS)) as usize
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
    }
}
