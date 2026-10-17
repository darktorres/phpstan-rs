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
}
